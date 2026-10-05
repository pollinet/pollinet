# pollinet-sdk-ios

Swift Package for the PolliNet offline-first Solana transaction propagation
network — the iOS counterpart of `pollinet-sdk` (Android). Same Rust core, same
wire protocol, same public API shape.

```
PolliNetRust.xcframework   Rust core (staticlib + cbindgen C header)
Sources/PolliNetSDK/
  PolliNetSDK.swift        High-level API (mirrors PolliNetSDK.kt method-for-method)
  PolliNetFFI.swift        C-ABI plumbing (JSON FfiResult envelope)
  Models.swift             Wire-exact Codable DTOs
  BleController.swift      CoreBluetooth driver (mirrors BleService.kt)
  MultipeerController.swift  MultipeerConnectivity transport (Wi-Fi Direct counterpart)
  BackgroundMaintenance.swift  BGTaskScheduler jobs (WorkManager counterpart)
  KeystoreManager.swift    Keychain/Secure Enclave keys (device identity only)
```

## Building the Rust core

```sh
../scripts/build_ios.sh    # cargo (device+sim+macOS) → cbindgen → xcframework
```

**The xcframework lags source exactly like the committed Android `.so` does —
re-run the script after ANY change under `src/ffi/` (especially `ios.rs` /
`common.rs`).** Verify exports with:
`nm -g PolliNetRust.xcframework/ios-arm64/libpollinet.a | grep -c "T _pollinet_"` → 68.

`POLLICORE_URL` (and optional `POLLICORE_PUBKEY`) are baked in at Rust compile
time from `../.env`, same as Android.

## Quick start

```swift
let sdk = try await PolliNetSDK.initialize(config: SdkConfig(
    rpcUrl: "https://api.devnet.solana.com",
    storageDirectory: documentsPath, encryptionKey: "…", walletAddress: "…"
))
let ble = BleController(sdk: sdk)
ble.start(); ble.startScanning(); ble.startAdvertising()

// iOS↔iOS high-bandwidth transport, sharing the BLE engine (cross-radio dedup):
if let wifiSdk = sdk.makeSharedWifiDirectSDK() {
    let mpc = MultipeerController(sdk: wifiSdk)
    mpc.start()
}
```

App requirements (see the PolliNetExample project):
- Info.plist: `NSBluetoothAlwaysUsageDescription`, `NSLocalNetworkUsageDescription`,
  `NSBonjourServices` = `_pollinet-mesh._tcp`, background modes
  `bluetooth-central` + `bluetooth-peripheral`,
  `BGTaskSchedulerPermittedIdentifiers` = `xyz.pollinet.retry`, `xyz.pollinet.cleanup`.
- Register `BackgroundMaintenance.register(sdkProvider:)` before launch finishes;
  call `scheduleAll()` when entering background; run
  `runRetryPass`/`runCleanupPass` on every foreground activation (BGTasks are
  best-effort on iOS).

## Platform differences vs Android (by design)

| Android | iOS |
|---|---|
| Foreground service keeps radios hot | Background modes `bluetooth-central`/`peripheral`; **backgrounded advertising lands in the overflow area, invisible to Android scanners** — iOS relays are Android-discoverable only while foregrounded |
| Wi-Fi Direct (`WifiDirectService`) | MultipeerConnectivity — iOS↔iOS only; Android↔iOS high-bandwidth is not possible (cross-OS traffic rides BLE). Same `[u32 len][type][payload]` framing, so a LAN/TCP bridge is drop-in later |
| `BootReceiver` auto-start on boot | Not possible — iOS apps cannot launch at boot |
| WorkManager periodic jobs | `BGTaskScheduler` (best-effort) + foreground-activation passes |
| Battery-optimization exemption | No equivalent |
| Peer id from MAC address | Peer id from CoreBluetooth per-device UUID (compact id = first 4 bytes of SHA-256, local bookkeeping only) |
| Android Keystore P-256 (StrongBox) | Keychain / Secure Enclave P-256 — same Ed25519 caveat: intents are signed by a wallet-provided Ed25519 key, never by these device keys |

Known parity gap carried over from Android: Kotlin declares
`getConfirmationQueueSize`/`getQueueMetrics` externals with **no Rust
implementation** (they would crash if called). They are intentionally absent
from the Swift API until the Rust side grows them.

## Tests

`swift test` runs the suite on the Mac host against the real Rust engine (the
xcframework's `macos-arm64` slice). BLE/Multipeer paths need physical devices —
see `plan-ios-checklist.md` phases 3–6.
