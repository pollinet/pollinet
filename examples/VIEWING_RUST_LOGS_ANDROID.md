# Viewing Rust Logs in Android Terminal

## Current Setup

Rust logs in the Android app are configured with:
- **Tag**: `PolliNet-Rust`
- **Log Level**: `Debug` (configured in `src/ffi/android.rs`)
- **Logger**: `android_logger` (bridges Rust `log` macros to Android logcat)

## Viewing Logs with adb logcat

### Basic Command

```bash
# View all PolliNet-Rust logs
adb logcat -s PolliNet-Rust:D

# Or if you have multiple devices:
adb -s DEVICE_ID logcat -s PolliNet-Rust:D
```

### Filter by Multiple Tags

```bash
# View both Rust and Kotlin logs
adb logcat -s PolliNet-Rust:D PolliNet.BLE:D

# View all PolliNet-related logs (any tag containing "PolliNet")
adb logcat | grep -i pollinet
```

### With Timestamps

```bash
# View with timestamps
adb logcat -v time -s PolliNet-Rust:D
```

### Clear and Start Fresh

```bash
# Clear logcat buffer, then start viewing
adb logcat -c && adb logcat -s PolliNet-Rust:D
```

### Continuous Monitoring

```bash
# Keep watching (useful for real-time debugging)
adb logcat -s PolliNet-Rust:D | grep -E "push_received_transaction|Generated transaction ID|reassembled successfully"
```

## Important Note: tracing vs log Macros

**Current Issue:**
- Your code uses `tracing::debug!()` macros
- But `android_logger` only bridges `log::debug!()` macros to logcat
- `tracing::debug!()` logs might NOT appear in logcat unless there's a bridge

### Solutions

#### Option 1: Use `log` Macros (Recommended for Android)

Change `tracing::debug!()` to `log::debug!()` in your code:

```rust
// BEFORE:
tracing::debug!("🆔 Generated transaction ID: {}", tx_id);

// AFTER:
use log::debug;
debug!("🆔 Generated transaction ID: {}", tx_id);
```

#### Option 2: Add tracing-to-log Bridge

Add `tracing-log` dependency to route tracing macros to log macros:

1. Add to `Cargo.toml`:
   ```toml
   [dependencies]
   tracing-log = "0.1"
   ```

2. Initialize in `src/ffi/android.rs`:
   ```rust
   use tracing_log::LogTracer;
   
   ANDROID_LOGGER_INIT.call_once(|| {
       #[cfg(feature = "android_logger")]
       {
           // Initialize log-to-android bridge first
           android_logger::init_once(
               android_logger::Config::default()
                   .with_max_level(log::LevelFilter::Debug)
                   .with_tag("PolliNet-Rust"),
           );
           
           // Then bridge tracing to log
           let _ = LogTracer::init();
           
           info!("🔧 Android logger and tracing bridge initialized");
       }
   });
   ```

#### Option 3: Keep Using tracing, But Check if It's Already Bridged

The code might already have a bridge if `tracing_subscriber::fmt()` is initialized. However, `fmt()` writes to stdout/stderr, not logcat. For Android, you need the `tracing-log` bridge.

## Recommended Approach

For Android apps, use `log` macros directly since they're already bridged to logcat:

```rust
use log::{debug, info, warn, error};

// Instead of:
tracing::debug!("🆔 Generated transaction ID: {}", tx_id);

// Use:
debug!("🆔 Generated transaction ID: {}", tx_id);
```

## Quick Test

To verify logs are working:

1. **Add a test log in `push_received_transaction()`:**
   ```rust
   use log::info;
   info!("✅ TEST: push_received_transaction called with {} bytes", tx_bytes.len());
   ```

2. **View logs:**
   ```bash
   adb logcat -s PolliNet-Rust:D | grep "TEST"
   ```

3. **Trigger the function** (send a transaction)

4. **Check if log appears** - if yes, logging works! If no, you need to add the tracing-to-log bridge.

## Example: View Specific Function Logs

```bash
# View all logs from push_received_transaction
adb logcat -s PolliNet-Rust:D | grep "push_received_transaction"

# View transaction ID generation
adb logcat -s PolliNet-Rust:D | grep "Generated transaction ID"

# View reassembly logs
adb logcat -s PolliNet-Rust:D | grep -E "reassembled|fragment"
```

## Multiple Devices

If you have multiple devices connected:

```bash
# List devices
adb devices

# View logs from specific device
adb -s 0B171JECB15746 logcat -s PolliNet-Rust:D
```

## Log Levels

The logger is configured with `LevelFilter::Debug`, so you'll see:
- `error!` - Errors
- `warn!` - Warnings  
- `info!` - Info messages
- `debug!` - Debug messages
- `trace!` - Trace messages (most verbose)

To see only errors and warnings:
```bash
adb logcat -s PolliNet-Rust:W
```

