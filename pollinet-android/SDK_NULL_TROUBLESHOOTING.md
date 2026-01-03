# Troubleshooting: "SDK is null" Error

**Issue**: Getting `⚠️ sendNextOutbound: SDK is null` when GATT connection established
**Root Cause**: BleService SDK not initialized before attempting to send data
**Status**: Fix applied, needs app restart

---

## 🔍 **Why This Happens**

### The Architecture
```
┌─────────────┐         ┌──────────────┐
│  MainActivity│         │  BleService  │
│             │         │  (Foreground)│
│  Has SDK ✅ │         │  Has SDK ❓  │
└─────────────┘         └──────────────┘
       │                       │
       │                       │
    UI SDK              BLE Operations SDK
  (for RPC)           (for mesh relay)
```

**Two separate SDK instances are needed!**

### The Problem
The **BleService** is a **foreground service** that:
1. Starts when app launches
2. Keeps running even after app closes
3. Survives in background for days

**If you install a new version but don't restart the service, it keeps running the OLD code!**

---

## ✅ **Step-by-Step Fix**

### Step 1: Force Stop Old Service
```bash
adb shell am force-stop xyz.pollinet.android
```

This **kills** the old BleService instance completely.

### Step 2: Launch App Fresh
```bash
adb shell am start -n xyz.pollinet.android/.MainActivity
```

This starts a **new** BleService with our SDK initialization fix.

### Step 3: Verify SDK Initialization
**On the app**, check the logs screen. You should see:
```
[19:35:01] ✅ BLE Service SDK initialized successfully
[19:35:01] ✅ Bluetooth state monitor registered
[19:35:01] 🚀 Event-driven worker started
```

**If you DON'T see these**, the SDK initialization failed. Jump to [Debugging Section](#debugging).

---

## 🧪 **Testing the Fix**

### Test 1: Check SDK is Initialized
**Before connecting**, look for:
```
✅ BLE Service SDK initialized successfully
```

If you see this, SDK is ready! ✅

### Test 2: Start Operations
1. Click **"Start Advertise"**
2. Wait for: `✅ Advertising started`
3. Connect from peer device
4. Click **"Queue 1KB Sample Tx"**

### Test 3: Verify Sending Works
You should now see (instead of "SDK is null"):
```
🧪 Queueing sample transaction (1024 bytes)
📤 Queued 5 fragments for tx abc123...
➡️ Sending fragment (237B)
✅ Wrote 237B to [peer address]
```

**No more "SDK is null" errors!** ✅

---

## 🔍 **Debugging**

### If SDK Initialization Still Fails

#### Check 1: Is the service actually restarted?
Look for our new log message:
```
✅ Bluetooth state monitor registered - will handle BT on/off gracefully
```

**If you see this**: Service restarted with new code ✅
**If you DON'T see this**: Service is still old version ❌

**Solution**: Try force-stopping again, or reboot device.

#### Check 2: Is initialization being called?
The DiagnosticsScreen should call `initializeSdk` when service connects.

**How to verify**:
1. Close app completely
2. Launch app
3. Check logs immediately for:
   ```
   ✅ BLE Service SDK initialized successfully
   ```
   OR
   ```
   ❌ Failed to initialize BLE Service SDK: [error message]
   ```

#### Check 3: Is there an RPC error?
If you see:
```
❌ Failed to initialize BLE Service SDK: network error
```

**Possible causes**:
- Device has no internet connection
- RPC endpoint is down
- API key expired

**Solution**: Check device internet, try different RPC endpoint.

---

## 🛠️ **Manual Fix (If Needed)**

If automatic fix doesn't work, manually initialize SDK:

### Option 1: Initialize from App UI
Add an "Initialize SDK" button in the app that calls:
```kotlin
scope.launch {
    bleService?.initializeSdk(
        SdkConfig(
            rpcUrl = "https://devnet.helius-rpc.com/?api-key=...",
            enableLogging = true,
            logLevel = "info",
            storageDirectory = context.filesDir.absolutePath
        )
    )?.onSuccess {
        // Show success message
    }?.onFailure { e ->
        // Show error
    }
}
```

### Option 2: Initialize in BleService onCreate
Move SDK initialization to `BleService.onCreate()` so it always initializes:

```kotlin
override fun onCreate() {
    super.onCreate()
    
    // ... existing code ...
    
    // Auto-initialize SDK
    serviceScope.launch {
        initializeSdk(
            SdkConfig(
                rpcUrl = "https://devnet.helius-rpc.com/?api-key=...",
                enableLogging = true,
                logLevel = "info",
                storageDirectory = filesDir.absolutePath
            )
        ).onSuccess {
            appendLog("✅ SDK auto-initialized in service onCreate")
        }
    }
}
```

---

## 📝 **What We Changed**

### File: DiagnosticsScreen.kt

**Before**:
```kotlin
bleService?.initializeSdk(
    SdkConfig(
        rpcUrl = null,  // ❌ Problem!
        // ...
    )
)
```

**After**:
```kotlin
bleService?.initializeSdk(
    SdkConfig(
        rpcUrl = "https://devnet.helius-rpc.com/?api-key=...",  // ✅ Fixed!
        enableLogging = true,
        logLevel = "info",
        storageDirectory = context.filesDir.absolutePath
    )
)?.onSuccess {
    addLog("✅ BLE Service SDK initialized successfully")
}?.onFailure { e ->
    addLog("❌ Failed to initialize BLE Service SDK: ${e.message}")
}
```

---

## 🎯 **Quick Commands**

### Force Stop & Restart
```bash
# Stop old service
adb shell am force-stop xyz.pollinet.android

# Launch fresh
adb shell am start -n xyz.pollinet.android/.MainActivity
```

### Check If It Worked
```bash
# Watch logs live
adb logcat | grep "BLE Service SDK initialized"
```

You should see:
```
✅ BLE Service SDK initialized successfully
```

Within 1-2 seconds of app launch.

---

## ⚠️ **Important Notes**

### Why Foreground Services Are Tricky
- They survive app closure (by design)
- Installing new APK doesn't restart them
- You must **force-stop** to get fresh service

### How to Avoid This in Future
Always **force-stop before testing new builds**:
```bash
adb shell am force-stop xyz.pollinet.android
./gradlew :app:installDebug
adb shell am start -n xyz.pollinet.android/.MainActivity
```

Or use this one-liner:
```bash
adb shell am force-stop xyz.pollinet.android && \
./gradlew :app:installDebug && \
adb shell am start -n xyz.pollinet.android/.MainActivity
```

---

## ✅ **Expected Result**

After force-stop and restart, you should see:

### On App Launch (within 2 seconds)
```
[19:35:01] ✅ BLE Service SDK initialized successfully
[19:35:01] ✅ Bluetooth state monitor registered
[19:35:01] 🚀 Event-driven worker started (battery-optimized)
```

### On Transaction Send (after GATT connection)
```
[19:35:15] 🧪 Queueing sample transaction (1024 bytes)
[19:35:15] 📤 Queued 5 fragments for tx 8a3f2b1c...
[19:35:15] 🚀 Starting sending loop
[19:35:15] ➡️ Sending fragment (237B)
[19:35:15] ✅ Wrote 237B to AA:BB:CC:DD:EE:FF
```

**No "SDK is null" errors!** ✅

---

## 🚀 **Try It Now!**

Your app should now be running with:
1. ✅ Fresh BleService instance
2. ✅ SDK properly initialized with RPC
3. ✅ All 5 critical fixes active
4. ✅ Ready to send transactions

**Test it and let me know if you see the success messages!** 🎯

---

**Fixed**: December 30, 2025
**Status**: ✅ Applied & Installed
**Next**: Test on device

