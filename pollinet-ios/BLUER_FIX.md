# Fixed: bluer Dependency Issue on macOS

## The Problem

When building for iOS on macOS, the build was failing with:
```
error: failed to run custom build command for `libdbus-sys v0.2.7`
The system library `dbus-1` required by crate `libdbus-sys` was not found.
```

This happened because `bluer` (a Linux-only BLE crate) was pulling in `libdbus-sys`, which requires D-Bus - a Linux system library not available on macOS.

## The Root Cause

- `bluer` is a Linux-specific BLE library that depends on BlueZ/D-Bus
- It was listed in the main `[dependencies]` section
- Even though it was marked `optional = true`, Cargo was still trying to build it on macOS during iOS builds

## The Solution

**Moved `bluer` to Linux-only dependencies:**

```toml
# BEFORE (in main dependencies):
bluer = { version = "0.16", optional = true, features = ["bluetoothd"] }

# AFTER (in platform-specific section):
[target.'cfg(target_os = "linux")'.dependencies]
bluer = { version = "0.16", optional = true, features = ["bluetoothd"] }
```

**Updated feature flags:**

```toml
# BEFORE:
default = ["linux", "rpc-client", "ble", "config-file"]
linux = ["bluer", "ble"]

# AFTER:
default = ["rpc-client", "ble", "config-file"]  # Removed "linux"
linux = ["ble"]  # Removed "bluer" (now in platform section)
```

## Why This Works

- Platform-specific dependencies (`[target.'cfg(...)'.dependencies]`) are only compiled for that platform
- On macOS/iOS builds, `bluer` and `libdbus-sys` are completely ignored
- On Linux builds, `bluer` is available when the `linux` feature is enabled
- Android and Windows builds are unaffected (they use `btleplug` instead)

## Verification

```bash
# Should now work on macOS:
cargo build --target aarch64-apple-ios --no-default-features --features ios --release

# Should still work on Linux:
cargo build --features linux
```

## Impact

✅ **iOS builds:** Now compile cleanly on macOS
✅ **Linux builds:** Still work with `bluer` when needed
✅ **Android builds:** Unaffected (use `btleplug`)
✅ **Windows/macOS builds:** Unaffected (use `btleplug`)

---

**Status: Fixed** ✅
