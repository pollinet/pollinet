# iOS FFI BLE Functions - Not Needed

## The Problem

Several iOS FFI functions reference `crate::ble::` types, but BLE is conditionally compiled out for iOS builds. These functions fail to compile because they try to use types that don't exist in iOS builds.

## Affected Functions (in `src/ffi/ios.rs`):

1. `pollinet_fragment_transaction` (line 1957)
2. `pollinet_reconstruct_transaction` (line 2032, 2045, 2054)
3. `pollinet_get_fragmentation_stats` (line 2089)
4. `pollinet_broadcast_transaction` (line 2147, 2154)
5. `pollinet_get_health_snapshot` (line 2234)
6. `pollinet_push_outbound_fragments` (line 2745, 2760)

## Why These Functions Are Not Needed on iOS

**iOS uses CoreBluetooth directly** from Swift, not the Rust BLE transport. The iOS app handles:
- BLE advertising and scanning
- Fragmentation (if needed, in Swift)
- Broadcasting transactions over BLE
- Health monitoring

The Rust iOS FFI only provides:
- Transaction building (unsigned)
- Signature operations
- Offline bundle management
- Queue management (for caching)

## The Solution

**Option 1: Remove These Functions (RECOMMENDED)**
Delete these 6 BLE-related functions from `src/ffi/ios.rs`. They're not used by the iOS app anyway.

**Option 2: Make Them Return Errors**
Wrap each function body to return an error explaining BLE is not available:

```rust
#[cfg(feature = "ios")]
#[no_mangle]
pub extern "C" fn pollinet_fragment_transaction(...) -> *mut c_char {
    let error = FfiResult::<()>::error("BLE operations not available on iOS. Use CoreBluetooth directly.");
    create_result_string(Ok(serde_json::to_string(&error).unwrap()))
}
```

## Recommendation

**Remove the functions entirely.** The iOS SDK doesn't need them, and they just add confusion. The core transaction and signature operations work fine without them.

If the user wants them as placeholders for future use, we can keep them with error stubs.
