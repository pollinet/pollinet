# PolliNet iOS SDK Implementation Guide

## Table of Contents
1. [Overview](#overview)
2. [Architecture Overview](#architecture-overview)
3. [Core Components Mapping](#core-components-mapping)
4. [FFI Bridge Implementation](#ffi-bridge-implementation)
5. [BLE Service Implementation](#ble-service-implementation)
6. [SDK API Implementation](#sdk-api-implementation)
7. [Data Models and Types](#data-models-and-types)
8. [Background Processing](#background-processing)
9. [Permissions and Configuration](#permissions-and-configuration)
10. [Testing Strategy](#testing-strategy)
11. [Implementation Checklist](#implementation-checklist)

---

## Overview

This document provides a comprehensive guide for implementing the PolliNet iOS SDK in Swift, replicating all functionality from the Android SDK. The iOS SDK must support:

- **BLE Mesh Networking**: Advertising, scanning, and peer connections
- **Transaction Fragmentation**: Splitting large transactions into BLE-sized fragments
- **Offline Transaction Creation**: Creating Solana transactions without internet
- **MWA Integration**: Mobile Wallet Adapter support for secure signing
- **Autonomous Relay System**: Auto-submitting received transactions
- **Queue Management**: Priority-based outbound, retry, and confirmation queues
- **Background Processing**: Battery-optimized background operations

---

## Architecture Overview

### Android Architecture (Reference)
```
┌─────────────────────────────────────────────────────────┐
│                    Android Application                  │
├─────────────────────────────────────────────────────────┤
│  PolliNetSDK.kt (High-level API)                        │
│  ├── JSON Serialization                                 │
│  ├── Coroutine Integration                              │
│  └── Error Handling                                     │
├─────────────────────────────────────────────────────────┤
│  PolliNetFFI.kt (JNI Bridge)                            │
│  └── Native Method Declarations                         │
├─────────────────────────────────────────────────────────┤
│  BleService.kt (Foreground Service)                     │
│  ├── BLE GATT Server/Client                             │
│  ├── Scanning/Advertising                               │
│  ├── Fragment Transmission                              │
│  ├── Event-Driven Worker                                │
│  └── Background Task Management                         │
├─────────────────────────────────────────────────────────┤
│  KeystoreManager.kt (Optional)                          │
│  └── Android Keystore Integration                       │
└─────────────────────────────────────────────────────────┘
                    ↓ JNI
┌─────────────────────────────────────────────────────────┐
│              Rust Core (libpollinet.so)                 │
│  ├── FFI Module (android.rs)                            │
│  ├── Transport Layer (transport.rs)                     │
│  ├── BLE Mesh (ble/)                                    │
│  ├── Transaction Service                                │
│  └── Queue Manager                                      │
└─────────────────────────────────────────────────────────┘
```

### iOS Architecture (Target)
```
┌─────────────────────────────────────────────────────────┐
│                    iOS Application                      │
├─────────────────────────────────────────────────────────┤
│  PolliNetSDK.swift (High-level API)                     │
│  ├── JSON Codable Integration                           │
│  ├── async/await Integration                            │
│  └── Error Handling                                     │
├─────────────────────────────────────────────────────────┤
│  PolliNetFFI.swift (C Bridge)                           │
│  └── C Function Declarations                            │
├─────────────────────────────────────────────────────────┤
│  PolliNetBLEService.swift (Background Service)          │
│  ├── CoreBluetooth Manager                              │
│  ├── CBPeripheralManager (Advertising)                  │
│  ├── CBCentralManager (Scanning)                        │
│  ├── Fragment Transmission                              │
│  ├── Event-Driven Worker                                │
│  └── Background Task Management                         │
├─────────────────────────────────────────────────────────┤
│  PolliNetKeychainManager.swift (Optional)               │
│  └── iOS Keychain Integration                           │
└─────────────────────────────────────────────────────────┘
                    ↓ C FFI
┌─────────────────────────────────────────────────────────┐
│         Rust Core (libpollinet.a / XCFramework)         │
│  ├── FFI Module (ios.rs)                                │
│  ├── Transport Layer (transport.rs)                     │
│  ├── BLE Mesh (ble/)                                    │
│  ├── Transaction Service                                │
│  └── Queue Manager                                      │
└─────────────────────────────────────────────────────────┘
```

---

## Core Components Mapping

### 1. PolliNetSDK.kt → PolliNetSDK.swift

**Android (Kotlin):**
- Singleton pattern with `initialize()` factory
- Uses `Result<T>` for error handling
- Coroutines (`suspend fun`) for async operations
- JSON serialization with `kotlinx.serialization`

**iOS (Swift):**
- Class with static `initialize()` factory method
- Uses `Result<T, Error>` or `throws` for error handling
- `async/await` for async operations
- JSON with `Codable` protocol

**Key Methods to Implement:**
```swift
class PolliNetSDK {
    private let handle: Int64
    private let jsonEncoder: JSONEncoder
    private let jsonDecoder: JSONDecoder
    
    static func initialize(config: SdkConfig) async throws -> PolliNetSDK
    static func version() -> String
    func shutdown()
    
    // Transport API
    func pushInbound(data: Data) async throws
    func nextOutbound(maxLen: Int) async -> Data?
    func tick() async throws -> [String]
    func metrics() async throws -> MetricsSnapshot
    func clearTransaction(txId: String) async throws
    
    // Transaction Builders
    func createUnsignedTransaction(request: CreateUnsignedTransactionRequest) async throws -> String
    func createUnsignedSplTransaction(request: CreateUnsignedSplTransactionRequest) async throws -> String
    func createUnsignedVote(request: CastUnsignedVoteRequest) async throws -> String
    
    // Signature Helpers
    func prepareSignPayload(base64Tx: String) async -> Data?
    func applySignature(base64Tx: String, signerPubkey: String, signatureBytes: Data) async throws -> String
    func verifyAndSerialize(base64Tx: String) async throws -> String
    
    // Fragmentation
    func fragment(txBytes: Data, maxPayload: Int?) async throws -> FragmentList
    
    // Offline Bundle Management
    func prepareOfflineBundle(count: Int, senderKeypair: Data, bundleFile: String?) async throws -> OfflineTransactionBundle
    func createOfflineTransaction(senderKeypair: Data, nonceAuthorityKeypair: Data, recipient: String, amount: Int64) async throws -> String
    func submitOfflineTransaction(transactionBase64: String, verifyNonce: Bool, onSuccess: ((String) -> Void)?) async throws -> String
    func submitNonceAccountCreationAndCache(unsignedTransaction: UnsignedNonceTransaction, finalSignedTransactionBase64: String) async throws -> String
    
    // MWA Support
    func createUnsignedOfflineTransaction(senderPubkey: String, nonceAuthorityPubkey: String, recipient: String, amount: Int64) async throws -> String
    func createUnsignedOfflineSplTransaction(senderWallet: String, recipientWallet: String, mintAddress: String, amount: Int64, feePayer: String) async throws -> String
    func getTransactionMessageToSign(unsignedTransactionBase64: String) async throws -> String
    func getRequiredSigners(unsignedTransactionBase64: String) async throws -> [String]
    func createUnsignedNonceTransactions(count: Int, payerPubkey: String) async throws -> [UnsignedNonceTransaction]
    func cacheNonceAccounts(nonceAccounts: [String]) async throws -> Int
    func createUnsignedNonceAccountsAndCache(count: Int, payerPubkey: String, onCreated: (([String]) -> Void)?) async throws -> [UnsignedNonceTransaction]
    func cacheNonceAccountsAfterSubmission(transactions: [UnsignedNonceTransaction], successfulSignatures: [String]) async throws -> Int
    func refreshBlockhashInUnsignedTransaction(unsignedTxBase64: String) async throws -> String
    func refreshOfflineBundle() async throws -> Int
    func addNonceSignature(payerSignedTransactionBase64: String, nonceKeypairBase64: [String]) async throws -> String
    
    // BLE Mesh Operations
    func fragmentTransaction(transactionBytes: Data) async throws -> [FragmentData]
    func reconstructTransaction(fragments: [FragmentData]) async throws -> String
    func getFragmentationStats(transactionBytes: Data) async throws -> FragmentationStats
    func prepareBroadcast(transactionBytes: Data) async throws -> BroadcastPreparation
    
    // Autonomous Transaction Relay
    func pushReceivedTransaction(transactionBytes: Data) async throws -> PushResponse
    func nextReceivedTransaction() async throws -> ReceivedTransaction?
    func getReceivedQueueSize() async throws -> Int
    func getFragmentReassemblyInfo() async throws -> FragmentReassemblyInfoList
    func markTransactionSubmitted(transactionBytes: Data) async throws -> Bool
    func cleanupOldSubmissions() async throws -> Bool
    func debugOutboundQueue() async throws -> OutboundQueueDebug
    
    // Queue Management
    func pushOutboundTransaction(txBytes: Data, txId: String, fragments: [FragmentFFI], priority: Priority) async throws
    func popOutboundTransaction() async throws -> OutboundTransaction?
    func getOutboundQueueSize() async throws -> Int
    func addToRetryQueue(txBytes: Data, txId: String, error: String) async throws
    func popReadyRetry() async throws -> RetryItem?
    func getRetryQueueSize() async throws -> Int
    func queueConfirmation(txId: String, signature: String) async throws
    func popConfirmation() async throws -> Confirmation?
    func getConfirmationQueueSize() async throws -> Int
    func getQueueMetrics() async throws -> QueueMetrics
    func cleanupStaleFragments() async throws -> Int
    func cleanupExpired() async throws -> (confirmationsCleaned: Int, retriesCleaned: Int)
    
    // Queue Persistence
    func saveQueues() async throws
    func autoSaveQueues() async throws
}
```

### 2. PolliNetFFI.kt → PolliNetFFI.swift

**Android (JNI):**
```kotlin
object PolliNetFFI {
    init {
        System.loadLibrary("pollinet")
    }
    external fun init(configBytes: ByteArray): Long
    external fun version(): String
    external fun shutdown(handle: Long)
    // ... more external functions
}
```

**iOS (C FFI):**
```swift
import Foundation

class PolliNetFFI {
    // Load static library or framework
    static func loadLibrary() {
        // Load libpollinet.a or PolliNet.xcframework
    }
    
    // C function declarations (matching Rust exports)
    @_silgen_name("pollinet_init")
    static func init(_ configBytes: UnsafePointer<UInt8>, _ configLen: Int) -> Int64
    
    @_silgen_name("pollinet_version")
    static func version() -> UnsafePointer<CChar>
    
    @_silgen_name("pollinet_shutdown")
    static func shutdown(_ handle: Int64)
    
    // ... more C function declarations
}
```

**Rust FFI Functions to Export (ios.rs):**
```rust
#[no_mangle]
pub extern "C" fn pollinet_init(config_json: *const c_char, config_len: usize) -> i64 {
    // Parse JSON config, initialize SDK, return handle
}

#[no_mangle]
pub extern "C" fn pollinet_version() -> *const c_char {
    // Return version string
}

#[no_mangle]
pub extern "C" fn pollinet_shutdown(handle: i64) {
    // Cleanup SDK instance
}

// Transport API
#[no_mangle]
pub extern "C" fn pollinet_push_inbound(
    handle: i64,
    data: *const u8,
    data_len: usize
) -> *mut c_char {
    // Return JSON result
}

#[no_mangle]
pub extern "C" fn pollinet_next_outbound(
    handle: i64,
    max_len: usize,
    out_data: *mut u8,
    out_len: *mut usize
) -> i32 {
    // Return 1 if data available, 0 if empty
    // Write data to out_data, set out_len
}

// ... more FFI functions matching Android JNI interface
```

### 3. BleService.kt → PolliNetBLEService.swift

**Key Responsibilities:**
1. **BLE GATT Server/Client Management**
2. **Scanning and Advertising**
3. **Fragment Transmission**
4. **Connection Management**
5. **Event-Driven Worker**
6. **Background Task Management**

**iOS Implementation Structure:**
```swift
import CoreBluetooth
import Foundation
import Combine

class PolliNetBLEService: NSObject {
    // MARK: - Properties
    
    // CoreBluetooth Managers
    private var peripheralManager: CBPeripheralManager?
    private var centralManager: CBCentralManager?
    
    // GATT Service and Characteristics
    private let serviceUUID = CBUUID(string: "00001820-0000-1000-8000-00805f9b34fb")
    private let txCharacteristicUUID = CBUUID(string: "00001821-0000-1000-8000-00805f9b34fb")
    private let rxCharacteristicUUID = CBUUID(string: "00001822-0000-1000-8000-00805f9b34fb")
    
    // State Management
    @Published var connectionState: ConnectionState = .disconnected
    @Published var isAdvertising: Bool = false
    @Published var isScanning: Bool = false
    @Published var metrics: MetricsSnapshot?
    @Published var logs: [String] = []
    
    // SDK Instance
    var sdk: PolliNetSDK?
    
    // MTU Tracking
    private var currentMTU: Int = 185 // Default, updated on negotiation
    
    // Connection Management
    private var connectedPeripheral: CBPeripheral?
    private var connectedCentral: CBCentral?
    private var txCharacteristic: CBMutableCharacteristic?
    private var rxCharacteristic: CBMutableCharacteristic?
    
    // Sending State
    private var sendingTask: Task<Void, Never>?
    private let sendingMutex = NSLock()
    private let operationQueue = DispatchQueue(label: "com.pollinet.ble.operations")
    
    // Event-Driven Worker
    private var unifiedWorker: Task<Void, Never>?
    private let workChannel = AsyncChannel<WorkEvent>()
    
    // Background Task Management
    private var backgroundTaskID: UIBackgroundTaskIdentifier = .invalid
    private var backgroundTimer: Timer?
    
    // Network Monitoring
    private var networkMonitor: NWPathMonitor?
    private var networkQueue: DispatchQueue?
    
    // MARK: - Initialization
    
    override init() {
        super.init()
        setupBluetooth()
        setupNetworkMonitoring()
    }
    
    // MARK: - BLE Setup
    
    private func setupBluetooth() {
        peripheralManager = CBPeripheralManager(delegate: self, queue: nil)
        centralManager = CBCentralManager(delegate: self, queue: nil)
    }
    
    // MARK: - Public API
    
    func start() async throws {
        guard await requestBluetoothPermissions() else {
            throw PolliNetError.bluetoothPermissionDenied
        }
        
        // Initialize SDK if needed
        if sdk == nil {
            let config = SdkConfig(
                version: 1,
                rpcUrl: nil,
                enableLogging: true,
                logLevel: "info",
                storageDirectory: getStorageDirectory()
            )
            sdk = try await PolliNetSDK.initialize(config: config)
        }
        
        // Start alternating mesh mode
        startAlternatingMeshMode()
        
        // Start unified event worker
        startUnifiedEventWorker()
        
        // Start background tasks
        scheduleBackgroundTasks()
    }
    
    func stop() {
        stopAlternatingMeshMode()
        stopScanning()
        stopAdvertising()
        unifiedWorker?.cancel()
        cancelBackgroundTasks()
    }
    
    // MARK: - Scanning
    
    func startScanning() {
        guard let central = centralManager,
              central.state == .poweredOn,
              !isScanning else { return }
        
        let scanOptions: [String: Any] = [
            CBCentralManagerScanOptionAllowDuplicatesKey: false
        ]
        
        central.scanForPeripherals(
            withServices: [serviceUUID],
            options: scanOptions
        )
        
        isScanning = true
        appendLog("🔍 Started scanning for PolliNet devices")
    }
    
    func stopScanning() {
        centralManager?.stopScan()
        isScanning = false
        appendLog("🛑 Stopped scanning")
    }
    
    // MARK: - Advertising
    
    func startAdvertising() {
        guard let peripheral = peripheralManager,
              peripheral.state == .poweredOn,
              !isAdvertising else { return }
        
        // Create GATT service and characteristics
        setupGATTService()
        
        // Start advertising
        let advertisementData: [String: Any] = [
            CBAdvertisementDataServiceUUIDsKey: [serviceUUID],
            CBAdvertisementDataLocalNameKey: "PolliNet"
        ]
        
        peripheral.startAdvertising(advertisementData)
        isAdvertising = true
        appendLog("📡 Started advertising PolliNet service")
    }
    
    func stopAdvertising() {
        peripheralManager?.stopAdvertising()
        isAdvertising = false
        appendLog("🛑 Stopped advertising")
    }
    
    // MARK: - GATT Service Setup
    
    private func setupGATTService() {
        guard let peripheral = peripheralManager else { return }
        
        // Create TX characteristic (write, notify)
        txCharacteristic = CBMutableCharacteristic(
            type: txCharacteristicUUID,
            properties: [.write, .notify],
            value: nil,
            permissions: [.writeable]
        )
        
        // Create RX characteristic (read, notify)
        rxCharacteristic = CBMutableCharacteristic(
            type: rxCharacteristicUUID,
            properties: [.read, .notify],
            value: nil,
            permissions: [.readable]
        )
        
        // Create service
        let service = CBMutableService(type: serviceUUID, primary: true)
        service.characteristics = [txCharacteristic!, rxCharacteristic!]
        
        // Add service to peripheral manager
        peripheral.add(service)
    }
    
    // MARK: - Fragment Transmission
    
    func queueSignedTransaction(txBytes: Data, priority: Priority = .normal) async throws -> Int {
        guard let sdk = sdk else {
            throw PolliNetError.sdkNotInitialized
        }
        
        // Validate size
        if txBytes.count > MAX_TRANSACTION_SIZE {
            throw PolliNetError.transactionTooLarge
        }
        
        let maxPayload = max(currentMTU - 10, 20)
        
        // Fragment transaction
        let fragmentList = try await sdk.fragment(txBytes: txBytes, maxPayload: maxPayload)
        
        // Extract transaction ID from first fragment
        guard let firstFragment = fragmentList.fragments.first else {
            throw PolliNetError.invalidFragment
        }
        
        let txId = try extractTransactionId(from: firstFragment)
        
        // Convert to FragmentFFI format
        let fragmentsFFI = fragmentList.fragments.map { fragment in
            FragmentFFI(
                transactionId: txId,
                fragmentIndex: Int(fragment.index),
                totalFragments: Int(fragment.total),
                dataBase64: fragment.data
            )
        }
        
        // Push to outbound queue
        try await sdk.pushOutboundTransaction(
            txBytes: txBytes,
            txId: txId,
            fragments: fragmentsFFI,
            priority: priority
        )
        
        // Trigger event
        await workChannel.send(.outboundReady)
        
        // Start sending loop if needed
        ensureSendingLoopStarted()
        
        return fragmentList.fragments.count
    }
    
    // MARK: - Sending Loop
    
    private func ensureSendingLoopStarted() {
        guard sendingTask == nil || sendingTask?.isCancelled == true else { return }
        
        sendingTask = Task {
            while !Task.isCancelled {
                guard connectionState == .connected else {
                    try? await Task.sleep(nanoseconds: 1_000_000_000) // 1 second
                    continue
                }
                
                guard let sdk = sdk else { break }
                
                // Get next outbound fragment
                if let fragmentData = await sdk.nextOutbound(maxLen: currentMTU) {
                    await sendToGATT(data: fragmentData)
                } else {
                    // No data, wait a bit
                    try? await Task.sleep(nanoseconds: 100_000_000) // 100ms
                }
            }
        }
    }
    
    private func sendToGATT(data: Data) async {
        // Implementation depends on connection mode (central vs peripheral)
        if let peripheral = connectedPeripheral,
           let characteristic = findRemoteCharacteristic(for: peripheral) {
            // Central mode: write to remote characteristic
            peripheral.writeValue(data, for: characteristic, type: .withResponse)
        } else if let central = connectedCentral,
                  let characteristic = txCharacteristic {
            // Peripheral mode: notify central
            peripheralManager?.updateValue(data, for: characteristic, onSubscribedCentrals: [central])
        }
    }
    
    // MARK: - Event-Driven Worker
    
    private func startUnifiedEventWorker() {
        guard unifiedWorker == nil || unifiedWorker?.isCancelled == true else { return }
        
        unifiedWorker = Task {
            var lastCleanup = Date()
            var lastReceivedCheck = Date()
            
            while !Task.isCancelled {
                do {
                    // Wait for event or timeout (30 seconds)
                    let event = try await withTimeout(seconds: 30) {
                        await workChannel.receive()
                    }
                    
                    switch event {
                    case .outboundReady:
                        // Sending loop handles this
                        break
                        
                    case .receivedReady:
                        await processReceivedQueue()
                        lastReceivedCheck = Date()
                        
                    case .retryReady:
                        await processRetryQueue()
                        
                    case .confirmationReady:
                        await processConfirmationQueue()
                        
                    case .cleanupNeeded:
                        await processCleanup()
                        lastCleanup = Date()
                    }
                } catch {
                    // Timeout or error - run fallback checks
                    let timeSinceLastCheck = Date().timeIntervalSince(lastReceivedCheck)
                    let timeSinceLastCleanup = Date().timeIntervalSince(lastCleanup)
                    
                    if timeSinceLastCheck > 10 {
                        await processReceivedQueue()
                        lastReceivedCheck = Date()
                    }
                    
                    if timeSinceLastCleanup > 300 {
                        await processCleanup()
                        lastCleanup = Date()
                    }
                }
            }
        }
    }
    
    // MARK: - Queue Processing
    
    private func processReceivedQueue() async {
        guard let sdk = sdk,
              hasInternetConnection() else { return }
        
        let queueSize = try? await sdk.getReceivedQueueSize()
        guard let size = queueSize, size > 0 else { return }
        
        var processedCount = 0
        var successCount = 0
        
        for _ in 0..<min(5, size) {
            guard let receivedTx = try? await sdk.nextReceivedTransaction() else { break }
            
            do {
                let signature = try await sdk.submitOfflineTransaction(
                    transactionBase64: receivedTx.transactionBase64,
                    verifyNonce: false
                )
                
                // Mark as submitted
                let txBytes = Data(base64Encoded: receivedTx.transactionBase64) ?? Data()
                _ = try? await sdk.markTransactionSubmitted(transactionBytes: txBytes)
                
                // Queue confirmation
                let txHash = txBytes.sha256().hexString
                try? await sdk.queueConfirmation(txId: txHash, signature: signature)
                await workChannel.send(.confirmationReady)
                
                successCount += 1
            } catch {
                // Add to retry queue if not stale
                if !isStaleTransactionError(error) {
                    _ = try? await sdk.addToRetryQueue(
                        txBytes: Data(base64Encoded: receivedTx.transactionBase64) ?? Data(),
                        txId: receivedTx.txId,
                        error: error.localizedDescription
                    )
                }
            }
            
            processedCount += 1
        }
        
        appendLog("📊 Processed \(processedCount) transactions (\(successCount) successful)")
    }
    
    private func processRetryQueue() async {
        // Similar to processReceivedQueue but for retries
    }
    
    private func processConfirmationQueue() async {
        // Send confirmations over BLE
    }
    
    private func processCleanup() async {
        guard let sdk = sdk else { return }
        
        _ = try? await sdk.cleanupStaleFragments()
        _ = try? await sdk.cleanupExpired()
    }
    
    // MARK: - Alternating Mesh Mode
    
    private func startAlternatingMeshMode() {
        Task {
            while !Task.isCancelled {
                // Scan for 8 seconds
                await MainActor.run {
                    startScanning()
                }
                try? await Task.sleep(nanoseconds: 8_000_000_000)
                
                // Stop scanning
                await MainActor.run {
                    stopScanning()
                }
                
                // Advertise for 8 seconds
                await MainActor.run {
                    startAdvertising()
                }
                try? await Task.sleep(nanoseconds: 8_000_000_000)
                
                // Stop advertising
                await MainActor.run {
                    stopAdvertising()
                }
            }
        }
    }
    
    private func stopAlternatingMeshMode() {
        // Cancel alternating task
    }
    
    // MARK: - Background Tasks
    
    private func scheduleBackgroundTasks() {
        // Use BGTaskScheduler for iOS background processing
        BGTaskScheduler.shared.register(
            forTaskWithIdentifier: "com.pollinet.retry",
            using: nil
        ) { task in
            self.handleRetryTask(task: task as! BGProcessingTask)
        }
        
        BGTaskScheduler.shared.register(
            forTaskWithIdentifier: "com.pollinet.cleanup",
            using: nil
        ) { task in
            self.handleCleanupTask(task: task as! BGProcessingTask)
        }
    }
    
    private func cancelBackgroundTasks() {
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: "com.pollinet.retry")
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: "com.pollinet.cleanup")
    }
    
    // MARK: - Network Monitoring
    
    private func setupNetworkMonitoring() {
        networkMonitor = NWPathMonitor()
        networkQueue = DispatchQueue(label: "com.pollinet.network")
        
        networkMonitor?.pathUpdateHandler = { [weak self] path in
            if path.status == .satisfied {
                // Network available - trigger work
                Task {
                    await self?.workChannel.send(.receivedReady)
                    await self?.workChannel.send(.retryReady)
                }
            }
        }
        
        networkMonitor?.start(queue: networkQueue!)
    }
    
    // MARK: - Helper Methods
    
    private func appendLog(_ message: String) {
        DispatchQueue.main.async {
            let timestamp = DateFormatter.logFormatter.string(from: Date())
            self.logs.append("[\(timestamp)] \(message)")
            print("[PolliNet] \(message)")
        }
    }
    
    private func hasInternetConnection() -> Bool {
        return networkMonitor?.currentPath.status == .satisfied
    }
    
    private func requestBluetoothPermissions() async -> Bool {
        // iOS handles BLE permissions automatically
        // Just check if Bluetooth is available
        return await withCheckedContinuation { continuation in
            // Check Bluetooth state
            continuation.resume(returning: true)
        }
    }
    
    private func getStorageDirectory() -> String {
        let documentsPath = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let pollinetPath = documentsPath.appendingPathComponent("PolliNet")
        try? FileManager.default.createDirectory(at: pollinetPath, withIntermediateDirectories: true)
        return pollinetPath.path
    }
}

// MARK: - CBPeripheralManagerDelegate

extension PolliNetBLEService: CBPeripheralManagerDelegate {
    func peripheralManagerDidUpdateState(_ peripheral: CBPeripheralManager) {
        switch peripheral.state {
        case .poweredOn:
            appendLog("✅ Bluetooth powered on")
            if !isAdvertising {
                startAdvertising()
            }
        case .poweredOff:
            appendLog("📴 Bluetooth powered off")
            stopAdvertising()
        case .unauthorized:
            appendLog("❌ Bluetooth unauthorized")
        case .unsupported:
            appendLog("❌ Bluetooth unsupported")
        default:
            break
        }
    }
    
    func peripheralManager(_ peripheral: CBPeripheralManager, didAdd service: CBService, error: Error?) {
        if let error = error {
            appendLog("❌ Failed to add service: \(error.localizedDescription)")
        } else {
            appendLog("✅ GATT service added")
        }
    }
    
    func peripheralManagerDidStartAdvertising(_ peripheral: CBPeripheralManager, error: Error?) {
        if let error = error {
            appendLog("❌ Failed to start advertising: \(error.localizedDescription)")
            isAdvertising = false
        } else {
            appendLog("✅ Started advertising successfully")
            isAdvertising = true
        }
    }
    
    func peripheralManager(_ peripheral: CBPeripheralManager, central: CBCentral, didSubscribeTo characteristic: CBCharacteristic) {
        appendLog("📱 Central subscribed to characteristic")
        connectedCentral = central
        connectionState = .connected
    }
    
    func peripheralManager(_ peripheral: CBPeripheralManager, central: CBCentral, didUnsubscribeFrom characteristic: CBCharacteristic) {
        appendLog("📱 Central unsubscribed")
        connectedCentral = nil
        connectionState = .disconnected
    }
    
    func peripheralManager(_ peripheral: CBPeripheralManager, didReceiveWrite requests: [CBATTRequest]) {
        for request in requests {
            guard let data = request.value else { continue }
            
            // Handle received data
            Task {
                await handleReceivedData(data: data)
            }
            
            // Respond to request
            peripheral.respond(to: request, withResult: .success)
        }
    }
}

// MARK: - CBCentralManagerDelegate

extension PolliNetBLEService: CBCentralManagerDelegate {
    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        switch central.state {
        case .poweredOn:
            appendLog("✅ Bluetooth central ready")
            if !isScanning {
                startScanning()
            }
        case .poweredOff:
            appendLog("📴 Bluetooth powered off")
            stopScanning()
        default:
            break
        }
    }
    
    func centralManager(_ central: CBCentralManager, didDiscover peripheral: CBPeripheral, advertisementData: [String : Any], rssi RSSI: NSNumber) {
        appendLog("🔍 Discovered device: \(peripheral.identifier)")
        
        // Connect to discovered peripheral
        connectedPeripheral = peripheral
        peripheral.delegate = self
        central.connect(peripheral, options: nil)
        connectionState = .connecting
    }
    
    func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        appendLog("✅ Connected to peripheral")
        connectionState = .connected
        peripheral.discoverServices([serviceUUID])
    }
    
    func centralManager(_ central: CBCentralManager, didFailToConnect peripheral: CBPeripheral, error: Error?) {
        appendLog("❌ Failed to connect: \(error?.localizedDescription ?? "unknown")")
        connectionState = .disconnected
    }
    
    func centralManager(_ central: CBCentralManager, didDisconnectPeripheral peripheral: CBPeripheral, error: Error?) {
        appendLog("📴 Disconnected from peripheral")
        connectionState = .disconnected
        connectedPeripheral = nil
    }
}

// MARK: - CBPeripheralDelegate

extension PolliNetBLEService: CBPeripheralDelegate {
    func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        guard let services = peripheral.services else { return }
        
        for service in services {
            if service.uuid == serviceUUID {
                peripheral.discoverCharacteristics([txCharacteristicUUID, rxCharacteristicUUID], for: service)
            }
        }
    }
    
    func peripheral(_ peripheral: CBPeripheral, didDiscoverCharacteristicsFor service: CBService, error: Error?) {
        guard let characteristics = service.characteristics else { return }
        
        for characteristic in characteristics {
            if characteristic.uuid == rxCharacteristicUUID {
                // Subscribe to notifications
                peripheral.setNotifyValue(true, for: characteristic)
            }
        }
    }
    
    func peripheral(_ peripheral: CBPeripheral, didUpdateValueFor characteristic: CBCharacteristic, error: Error?) {
        guard let data = characteristic.value else { return }
        
        Task {
            await handleReceivedData(data: data)
        }
    }
    
    func peripheral(_ peripheral: CBPeripheral, didWriteValueFor characteristic: CBCharacteristic, error: Error?) {
        if let error = error {
            appendLog("❌ Write failed: \(error.localizedDescription)")
        } else {
            appendLog("✅ Write successful")
        }
    }
}

// MARK: - Supporting Types

enum ConnectionState {
    case disconnected
    case scanning
    case connecting
    case connected
    case error
}

enum WorkEvent {
    case outboundReady
    case receivedReady
    case retryReady
    case confirmationReady
    case cleanupNeeded
}

enum PolliNetError: Error {
    case sdkNotInitialized
    case bluetoothPermissionDenied
    case transactionTooLarge
    case invalidFragment
    case bluetoothUnavailable
}

extension DateFormatter {
    static let logFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss.SSS"
        return formatter
    }()
}

extension Data {
    func sha256() -> Data {
        var hash = [UInt8](repeating: 0, count: Int(CC_SHA256_DIGEST_LENGTH))
        self.withUnsafeBytes { bytes in
            _ = CC_SHA256(bytes.baseAddress, CC_LONG(self.count), &hash)
        }
        return Data(hash)
    }
    
    var hexString: String {
        return map { String(format: "%02x", $0) }.joined()
    }
}

// AsyncChannel implementation (simplified)
actor AsyncChannel<T> {
    private var continuations: [CheckedContinuation<T, Error>] = []
    private var values: [T] = []
    
    func send(_ value: T) async {
        if let continuation = continuations.popFirst() {
            continuation.resume(returning: value)
        } else {
            values.append(value)
        }
    }
    
    func receive() async throws -> T {
        if let value = values.popFirst() {
            return value
        } else {
            return try await withCheckedThrowingContinuation { continuation in
                continuations.append(continuation)
            }
        }
    }
}

func withTimeout<T>(seconds: TimeInterval, operation: @escaping () async throws -> T) async throws -> T? {
    return try await withThrowingTaskGroup(of: T?.self) { group in
        group.addTask {
            return try await operation()
        }
        
        group.addTask {
            try await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
            return nil
        }
        
        let result = try await group.next()
        group.cancelAll()
        return result
    }
}
```

### 4. KeystoreManager.kt → PolliNetKeychainManager.swift

**iOS Keychain Implementation:**
```swift
import Security
import Foundation

class PolliNetKeychainManager {
    private let service = "xyz.pollinet.sdk"
    
    func generateKeyPair(alias: String) throws -> Data {
        // Generate Ed25519 keypair using CryptoKit or similar
        // Store in Keychain
        // Return public key
    }
    
    func sign(alias: String, data: Data) throws -> Data {
        // Retrieve private key from Keychain
        // Sign data
        // Return signature
    }
    
    func getPublicKey(alias: String) throws -> Data {
        // Retrieve public key from Keychain
    }
    
    func deleteKey(alias: String) throws {
        // Delete key from Keychain
    }
    
    func keyExists(alias: String) -> Bool {
        // Check if key exists in Keychain
    }
}
```

---

## Data Models and Types

### Swift Codable Models

```swift
// MARK: - Configuration

struct SdkConfig: Codable {
    let version: Int
    let rpcUrl: String?
    let enableLogging: Bool
    let logLevel: String?
    let storageDirectory: String?
}

// MARK: - Transaction Builders

struct CreateUnsignedTransactionRequest: Codable {
    let version: Int
    let sender: String
    let recipient: String
    let feePayer: String
    let amount: Int64
    let nonceAccount: String
}

struct CreateUnsignedSplTransactionRequest: Codable {
    let version: Int
    let senderWallet: String
    let recipientWallet: String
    let feePayer: String
    let mintAddress: String
    let amount: Int64
    let nonceAccount: String
}

struct CastUnsignedVoteRequest: Codable {
    let version: Int
    let voter: String
    let proposalId: String
    let voteAccount: String
    let choice: UInt8
    let feePayer: String
    let nonceAccount: String
}

// MARK: - Fragmentation

struct Fragment: Codable {
    let id: String
    let index: UInt32
    let total: UInt32
    let data: String // base64
    let fragmentType: String
    let checksum: String // base64
}

struct FragmentList: Codable {
    let fragments: [Fragment]
}

struct FragmentData: Codable {
    let transactionId: String
    let fragmentIndex: Int
    let totalFragments: Int
    let dataBase64: String
}

struct FragmentFFI: Codable {
    let transactionId: String
    let fragmentIndex: Int
    let totalFragments: Int
    let dataBase64: String
}

struct FragmentationStats: Codable {
    let originalSize: Int
    let fragmentCount: Int
    let maxFragmentSize: Int
    let avgFragmentSize: Int
    let totalOverhead: Int
    let efficiency: Float
}

// MARK: - Offline Bundle

struct PrepareOfflineBundleRequest: Codable {
    let version: Int
    let count: Int
    let senderKeypairBase64: String
    let bundleFile: String?
}

struct CachedNonceData: Codable {
    let version: Int
    let nonceAccount: String
    let authority: String
    let blockhash: String
    let lamportsPerSignature: Int64
    let cachedAt: Int64
    let used: Bool
}

struct OfflineTransactionBundle: Codable {
    let version: Int
    let nonceCaches: [CachedNonceData]
    let maxTransactions: Int
    let createdAt: Int64
    
    func availableNonces() -> Int {
        nonceCaches.filter { !$0.used }.count
    }
    
    func usedNonces() -> Int {
        nonceCaches.filter { $0.used }.count
    }
    
    func totalNonces() -> Int {
        nonceCaches.count
    }
}

struct CreateOfflineTransactionRequest: Codable {
    let version: Int
    let senderKeypairBase64: String
    let nonceAuthorityKeypairBase64: String
    let recipient: String
    let amount: Int64
}

struct SubmitOfflineTransactionRequest: Codable {
    let version: Int
    let transactionBase64: String
    let verifyNonce: Bool
}

// MARK: - MWA Support

struct CreateUnsignedOfflineTransactionRequest: Codable {
    let version: Int
    let senderPubkey: String
    let nonceAuthorityPubkey: String
    let recipient: String
    let amount: Int64
}

struct CreateUnsignedOfflineSplTransactionRequest: Codable {
    let version: Int
    let senderWallet: String
    let recipientWallet: String
    let mintAddress: String
    let amount: Int64
    let feePayer: String
}

struct GetMessageToSignRequest: Codable {
    let version: Int
    let unsignedTransactionBase64: String
}

struct GetRequiredSignersRequest: Codable {
    let version: Int
    let unsignedTransactionBase64: String
}

struct CreateUnsignedNonceTransactionsRequest: Codable {
    let version: Int
    let count: Int
    let payerPubkey: String
}

struct UnsignedNonceTransaction: Codable {
    let unsignedTransactionBase64: String
    let nonceKeypairBase64: [String]
    let noncePubkey: [String]
}

struct CacheNonceAccountsRequest: Codable {
    let version: Int
    let nonceAccounts: [String]
}

struct CacheNonceAccountsResponse: Codable {
    let cachedCount: Int
}

struct RefreshOfflineBundleResponse: Codable {
    let refreshedCount: Int
}

struct AddNonceSignatureRequest: Codable {
    let version: Int
    let payerSignedTransactionBase64: String
    let nonceKeypairBase64: [String]
}

// MARK: - Metrics

struct MetricsSnapshot: Codable {
    let fragmentsBuffered: UInt32
    let transactionsComplete: UInt32
    let reassemblyFailures: UInt32
    let lastError: String
    let updatedAt: Int64
}

struct FragmentReassemblyInfo: Codable {
    let transactionId: String
    let totalFragments: Int
    let receivedFragments: Int
    let receivedIndices: [Int]
    let fragmentSizes: [Int]
    let totalBytesReceived: Int
}

struct FragmentReassemblyInfoList: Codable {
    let transactions: [FragmentReassemblyInfo]
}

// MARK: - BLE Mesh

struct BroadcastPreparation: Codable {
    let transactionId: String
    let fragmentPackets: [FragmentPacket]
}

struct FragmentPacket: Codable {
    let transactionId: String
    let fragmentIndex: Int
    let totalFragments: Int
    let packetBytes: String // Base64-encoded mesh packet
}

// MARK: - Autonomous Relay

struct PushResponse: Codable {
    let added: Bool
    let queueSize: Int
}

struct ReceivedTransaction: Codable {
    let txId: String
    let transactionBase64: String
    let receivedAt: Int64
}

// MARK: - Queue Management

enum Priority: String, Codable {
    case high = "HIGH"
    case normal = "NORMAL"
    case low = "LOW"
}

struct OutboundTransaction: Codable {
    let txId: String
    let originalBytes: String // base64
    let fragmentCount: Int
    let priority: Priority
    let createdAt: Int64
    let retryCount: Int
}

struct RetryItem: Codable {
    let txBytes: String // base64
    let txId: String
    let attemptCount: Int
    let lastError: String
    let nextRetryInSecs: Int64
    let ageSeconds: Int64
}

enum ConfirmationStatus: Codable {
    case success(signature: String)
    case failed(error: String)
    
    enum CodingKeys: String, CodingKey {
        case type
        case signature
        case error
    }
    
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)
        
        switch type {
        case "SUCCESS":
            let signature = try container.decode(String.self, forKey: .signature)
            self = .success(signature: signature)
        case "FAILED":
            let error = try container.decode(String.self, forKey: .error)
            self = .failed(error: error)
        default:
            throw DecodingError.dataCorruptedError(forKey: .type, in: container, debugDescription: "Unknown status type")
        }
    }
    
    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        
        switch self {
        case .success(let signature):
            try container.encode("SUCCESS", forKey: .type)
            try container.encode(signature, forKey: .signature)
        case .failed(let error):
            try container.encode("FAILED", forKey: .type)
            try container.encode(error, forKey: .error)
        }
    }
}

struct Confirmation: Codable {
    let txId: String
    let status: ConfirmationStatus
    let timestamp: Int64
    let relayCount: Int
}

struct QueueMetrics: Codable {
    let outboundSize: Int
    let outboundHighPriority: Int
    let outboundNormalPriority: Int
    let outboundLowPriority: Int
    let confirmationSize: Int
    let retrySize: Int
    let retryAvgAttempts: Float
}

struct PushOutboundRequest: Codable {
    let version: Int
    let txBytes: String
    let txId: String
    let fragments: [FragmentFFI]
    let priority: Priority
}

struct AddToRetryRequest: Codable {
    let version: Int
    let txBytes: String
    let txId: String
    let error: String
}

struct QueueConfirmationRequest: Codable {
    let version: Int
    let txId: String
    let signature: String
}

struct SuccessResponse: Codable {
    let success: Bool
}

struct QueueSizeResponse: Codable {
    let queueSize: Int
}

struct OutboundQueueDebug: Codable {
    let totalFragments: Int
    let fragments: [FragmentDebugInfo]
}

struct FragmentDebugInfo: Codable {
    let index: Int
    let size: Int
}

// MARK: - FFI Result

struct FfiResult<T: Codable>: Codable {
    let ok: Bool
    let data: T?
    let code: String?
    let message: String?
    
    func toSwiftResult() -> Result<T, PolliNetError> {
        if ok, let data = data {
            return .success(data)
        } else {
            return .failure(.ffiError(code: code ?? "UNKNOWN", message: message ?? "Unknown error"))
        }
    }
}

extension PolliNetError {
    case ffiError(code: String, message: String)
}
```

---

## Permissions and Configuration

### Info.plist Requirements

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <!-- Bluetooth permissions -->
    <key>NSBluetoothAlwaysUsageDescription</key>
    <string>PolliNet needs Bluetooth to connect with other devices and relay transactions</string>
    
    <key>NSBluetoothPeripheralUsageDescription</key>
    <string>PolliNet needs Bluetooth to advertise and receive transactions</string>
    
    <!-- Background modes -->
    <key>UIBackgroundModes</key>
    <array>
        <string>bluetooth-central</string>
        <string>bluetooth-peripheral</string>
        <string>processing</string>
    </array>
    
    <!-- Required device capabilities -->
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>armv7</string>
        <string>bluetooth-le</string>
    </array>
</dict>
</plist>
```

### Background Task Identifiers

Add to Info.plist:
```xml
<key>BGTaskSchedulerPermittedIdentifiers</key>
<array>
    <string>com.pollinet.retry</string>
    <string>com.pollinet.cleanup</string>
</array>
```

---

## Implementation Checklist

### Phase 1: Core SDK
- [ ] Create Xcode project with Swift Package Manager
- [ ] Set up Rust build system (cargo-xcode or manual)
- [ ] Implement Rust FFI module (ios.rs) matching android.rs
- [ ] Create PolliNetFFI.swift with C function declarations
- [ ] Implement PolliNetSDK.swift with all API methods
- [ ] Create all Codable data models
- [ ] Implement JSON serialization/deserialization
- [ ] Add error handling and Result types
- [ ] Write unit tests for SDK

### Phase 2: BLE Service
- [ ] Create PolliNetBLEService.swift
- [ ] Implement CBPeripheralManagerDelegate
- [ ] Implement CBCentralManagerDelegate
- [ ] Implement CBPeripheralDelegate
- [ ] Set up GATT service and characteristics
- [ ] Implement scanning functionality
- [ ] Implement advertising functionality
- [ ] Implement connection management
- [ ] Implement fragment transmission
- [ ] Implement fragment reception
- [ ] Add MTU negotiation
- [ ] Implement alternating mesh mode
- [ ] Add connection state management

### Phase 3: Event-Driven Architecture
- [ ] Implement AsyncChannel for events
- [ ] Create unified event worker
- [ ] Implement processReceivedQueue()
- [ ] Implement processRetryQueue()
- [ ] Implement processConfirmationQueue()
- [ ] Implement processCleanup()
- [ ] Add network monitoring
- [ ] Implement event triggering

### Phase 4: Background Processing
- [ ] Set up BGTaskScheduler
- [ ] Implement retry background task
- [ ] Implement cleanup background task
- [ ] Add background task registration
- [ ] Implement background task scheduling
- [ ] Test background execution

### Phase 5: Queue Management
- [ ] Implement pushOutboundTransaction()
- [ ] Implement popOutboundTransaction()
- [ ] Implement addToRetryQueue()
- [ ] Implement popReadyRetry()
- [ ] Implement queueConfirmation()
- [ ] Implement popConfirmation()
- [ ] Implement getQueueMetrics()
- [ ] Add queue persistence

### Phase 6: Offline Bundle
- [ ] Implement prepareOfflineBundle()
- [ ] Implement createOfflineTransaction()
- [ ] Implement submitOfflineTransaction()
- [ ] Implement cacheNonceAccounts()
- [ ] Implement refreshOfflineBundle()
- [ ] Add secure storage integration

### Phase 7: MWA Integration
- [ ] Implement createUnsignedOfflineTransaction()
- [ ] Implement createUnsignedOfflineSplTransaction()
- [ ] Implement getTransactionMessageToSign()
- [ ] Implement getRequiredSigners()
- [ ] Implement createUnsignedNonceTransactions()
- [ ] Implement addNonceSignature()
- [ ] Add MWA client integration example

### Phase 8: Testing
- [ ] Unit tests for SDK
- [ ] Unit tests for BLE service
- [ ] Integration tests
- [ ] End-to-end tests
- [ ] Performance tests
- [ ] Battery usage tests

### Phase 9: Documentation
- [ ] API documentation
- [ ] Usage examples
- [ ] Integration guide
- [ ] Troubleshooting guide

---

## Key Differences: Android vs iOS

### 1. Background Execution
- **Android**: Foreground Service with notification
- **iOS**: Background modes + BGTaskScheduler (limited execution time)

### 2. BLE Permissions
- **Android**: Runtime permissions (BLUETOOTH_SCAN, BLUETOOTH_CONNECT)
- **iOS**: Info.plist declarations (automatic, no runtime prompt)

### 3. Async Programming
- **Android**: Kotlin Coroutines (`suspend fun`)
- **iOS**: Swift async/await

### 4. JSON Serialization
- **Android**: kotlinx.serialization
- **iOS**: Codable protocol

### 5. Secure Storage
- **Android**: Android Keystore
- **iOS**: Keychain Services

### 6. FFI Bridge
- **Android**: JNI (Java Native Interface)
- **iOS**: C FFI (C function exports)

### 7. Lifecycle Management
- **Android**: Service lifecycle (onCreate, onDestroy)
- **iOS**: App lifecycle + background task management

---

## Testing Strategy

### Unit Tests
```swift
import XCTest
@testable import PolliNetSDK

class PolliNetSDKTests: XCTestCase {
    var sdk: PolliNetSDK!
    
    override func setUp() async throws {
        let config = SdkConfig(
            version: 1,
            rpcUrl: nil,
            enableLogging: true,
            logLevel: "debug",
            storageDirectory: nil
        )
        sdk = try await PolliNetSDK.initialize(config: config)
    }
    
    func testFragmentTransaction() async throws {
        let txBytes = Data(repeating: 0x42, count: 1024)
        let fragments = try await sdk.fragment(txBytes: txBytes, maxPayload: 200)
        XCTAssertGreaterThan(fragments.fragments.count, 0)
    }
    
    // More tests...
}
```

### Integration Tests
- Test BLE connection between two devices
- Test fragment transmission and reassembly
- Test queue operations
- Test offline transaction creation

### Performance Tests
- Measure fragmentation overhead
- Measure BLE transmission latency
- Measure queue processing throughput
- Measure battery usage

---

## Conclusion

This guide provides a comprehensive roadmap for implementing the PolliNet iOS SDK. The implementation should closely mirror the Android SDK functionality while adapting to iOS-specific APIs and constraints.

Key priorities:
1. **Functional Parity**: All Android features must work on iOS
2. **Battery Efficiency**: Optimize for iOS background execution limits
3. **User Experience**: Seamless integration with iOS apps
4. **Reliability**: Robust error handling and recovery

For questions or clarifications, refer to the Android SDK implementation as the reference implementation.
