# iOS Integration - Next Steps & Status

## ✅ Completed

1. **Rust FFI Implementation** (100%)
   - All 55 FFI functions implemented in `src/ffi/ios.rs`
   - C-compatible interface ready for Swift
   - Memory management with `pollinet_free_string()`

2. **Build Infrastructure**
   - `build-ios.sh` - Build script for iOS static libraries
   - Supports device (arm64) and simulator (x86_64 + arm64)

3. **Header Files**
   - `PolliNetFFI.h` - Complete C header with all 55 functions
   - `pollinet-ios-Bridging-Header.h` - Swift bridging header

4. **Swift Wrapper (Partial)**
   - `PolliNetSDK.swift` - Basic wrapper class with:
     - Initialization/shutdown
     - Transport API (pushInbound, nextOutbound, tick, metrics)
     - Basic transaction building (SOL, SPL transfers)
     - Error handling infrastructure

## 🚧 Remaining Work

### 1. Build & Test the Library

```bash
cd /Users/oghenekparoboreminokanju/pollinet
./build-ios.sh
```

This will:
- Add iOS targets to rustup if needed
- Build libraries for device and simulator
- Create universal binaries in `target/ios/`

**Note:** Requires network access for first-time dependency downloads.

### 2. Complete Xcode Project Configuration

1. **Add files to Xcode:**
   - Drag `PolliNetFFI.h` into project
   - Drag `PolliNetSDK.swift` into project  
   - Drag `pollinet-ios-Bridging-Header.h` into project

2. **Configure Build Settings:**
   - Set "Objective-C Bridging Header" to `pollinet-ios/pollinet-ios-Bridging-Header.h`
   - Add "Library Search Paths": `$(PROJECT_DIR)/../target/ios`
   - Link libraries conditionally (device vs simulator)

3. **Test Build:**
   - Clean build folder (Cmd+Shift+K)
   - Build (Cmd+B)
   - Fix any linking/path issues

### 3. Extend Swift Wrapper

The current `PolliNetSDK.swift` has ~15% of functions. Remaining to add:

#### Signature Operations
- `prepareSignPayload(base64Tx:)` → `Data`
- `applySignature(request:)` → `String`
- `verifyAndSerialize(base64Tx:)` → `String`

#### Governance Votes
- `castUnsignedVote(request:)` → `String`

#### Fragmentation
- `fragment(base64Tx:)` → `FragmentResponse`

#### Offline Bundle (7 functions)
- `prepareOfflineBundle(request:)` → `OfflineBundle`
- `createOfflineTransaction(request:)` → `String`
- `submitOfflineTransaction(request:)` → `String`
- `createUnsignedOfflineTransaction(request:)` → `String`
- `createUnsignedOfflineSplTransaction(request:)` → `String`
- `getTransactionMessageToSign(request:)` → `Data`
- `getRequiredSigners(request:)` → `[String]`

#### Nonce Management (5 functions)
- `createUnsignedNonceTransactions(request:)` → `[UnsignedNonceTransaction]`
- `cacheNonceAccounts(request:)` → `Int` (cached count)
- `refreshOfflineBundle()` → `Int` (refreshed count)
- `getAvailableNonce()` → `CachedNonceData?`
- `addNonceSignature(request:)` → `String`

#### Transaction Refresh
- `refreshBlockhashInUnsignedTransaction(base64Tx:)` → `String`

#### BLE Mesh (4 functions)
- `fragmentTransaction(data:)` → `[FragmentData]`
- `reconstructTransaction(fragments:)` → `String` (base64)
- `getFragmentationStats(data:)` → `FragmentationStats`
- `prepareBroadcast(data:)` → `BroadcastPreparation`

#### Health Monitoring (4 functions)
- `getHealthSnapshot()` → `HealthSnapshot`
- `recordPeerHeartbeat(peerId:)` → `Bool`
- `recordPeerLatency(peerId:latencyMs:)` -> `Bool`
- `recordPeerRssi(peerId:rssi:)` -> `Bool`

#### Received Queue (6 functions)
- `pushReceivedTransaction(data:)` -> `(added: Bool, queueSize: Int)`
- `nextReceivedTransaction()` -> `ReceivedTransaction?`
- `getReceivedQueueSize()` -> `Int`
- `getFragmentReassemblyInfo()` -> `[FragmentReassemblyInfo]`
- `markTransactionSubmitted(data:)` -> `Bool`
- `cleanupOldSubmissions()` -> `Bool`

#### Queue Management (13 functions)
- `debugOutboundQueue()` -> `QueueDebugInfo`
- `saveQueues()` -> `Bool`
- `autoSaveQueues()` -> `Bool`
- `pushOutboundTransaction(request:)` -> `Bool`
- `popOutboundTransaction()` -> `OutboundTransaction?`
- `getOutboundQueueSize()` -> `Int`
- `addToRetryQueue(request:)` -> `Bool`
- `popReadyRetry()` -> `RetryItem?`
- `getRetryQueueSize()` -> `Int`
- `cleanupExpired()` -> `(confirmations: Int, retries: Int)`
- `queueConfirmation(request:)` -> `Bool`
- `popConfirmation()` -> `Confirmation?`
- `cleanupStaleFragments()` -> `Int`

### 4. Add Swift Data Models

Add missing Swift structs matching Rust types:

```swift
// Fragment types
public struct FragmentData: Codable { ... }
public struct FragmentReassemblyInfo: Codable { ... }

// Health monitoring
public struct HealthSnapshot: Codable { ... }
public struct PeerHealth: Codable { ... }

// Queue types
public struct OutboundTransaction: Codable { ... }
public struct RetryItem: Codable { ... }
public struct Confirmation: Codable { ... }

// Offline bundle
public struct OfflineBundle: Codable { ... }
public struct UnsignedNonceTransaction: Codable { ... }
```

### 5. Add Async/Await Support

Wrap blocking FFI calls in Swift async functions:

```swift
public func createUnsignedTransactionAsync(request: CreateUnsignedTransactionRequest) async throws -> String {
    return try await Task {
        try createUnsignedTransaction(request: request).get()
    }.value
}
```

### 6. Add Comprehensive Error Handling

- Add more specific error types
- Add error recovery strategies
- Add logging integration

### 7. Testing

- Unit tests for Swift wrapper
- Integration tests with mock FFI
- End-to-end tests with actual Rust library

## 📝 Implementation Pattern

For each FFI function, follow this pattern:

```swift
public func functionName(request: RequestType) -> Result<ResponseType, Error> {
    guard handle >= 0 else {
        return .failure(PolliNetError.notInitialized)
    }
    
    return executeFFICall { jsonData in
        pollinet_function_name(handle, jsonData.baseAddress, jsonData.count)
    } request: request
}

// For functions without request params
public func functionName() -> ResponseType? {
    guard handle >= 0 else { return nil }
    
    guard let jsonPtr = pollinet_function_name(handle) else { return nil }
    defer { pollinet_free_string(jsonPtr) }
    
    let jsonString = String(cString: jsonPtr)
    guard let jsonData = jsonString.data(using: .utf8),
          let result = try? JSONDecoder().decode(FfiResult<ResponseType>.self, from: jsonData) else {
        return nil
    }
    
    switch result {
    case .ok(let data):
        return data
    case .err:
        return nil
    }
}
```

## 🔍 Reference

- **Rust FFI:** `src/ffi/ios.rs` - All 55 functions
- **C Header:** `pollinet-ios/PolliNetFFI.h` - Function signatures
- **Rust Types:** `src/ffi/types.rs` - Request/response structs
- **Android Reference:** `src/ffi/android.rs` - Similar implementation for reference

## 🎯 Priority Order

1. **Build & Link** - Get the library building and linking first
2. **Core Operations** - Transaction building, signatures, fragmentation
3. **Advanced Features** - Offline bundle, nonce management
4. **Queue Management** - Outbound, retry, confirmation queues
5. **Monitoring** - Health monitoring, metrics
6. **Polish** - Async/await, comprehensive error handling, tests

## ⚠️ Common Issues

1. **Undefined symbols** - Check library paths and linking flags
2. **Module not found** - Verify bridging header path
3. **Memory leaks** - Always use `defer { pollinet_free_string() }` for string returns
4. **JSON serialization** - Ensure Swift structs match Rust JSON schema exactly
