# PolliNet iOS Quick Start

## 🚀 Quick Setup (5 minutes)

### 1. Build the Rust Library

```bash
cd /Users/oghenekparoboreminokanju/pollinet
./build-ios.sh
```

This creates:
- `target/ios/libpollinet_device.a` (for devices)
- `target/ios/libpollinet_sim.a` (for simulators)

### 2. Add Files to Xcode

1. **Add Header File:**
   - Drag `PolliNetFFI.h` into Xcode project
   - ✅ Check "Copy items if needed"
   - ✅ Add to target: `pollinet-ios`

2. **Add Swift Wrapper:**
   - Drag `pollinet-ios/PolliNetSDK.swift` into Xcode project
   - ✅ Check "Copy items if needed"
   - ✅ Add to target: `pollinet-ios`

3. **Add Bridging Header:**
   - Drag `pollinet-ios-Bridging-Header.h` into Xcode project
   - ✅ Check "Copy items if needed"
   - ✅ Add to target: `pollinet-ios`

### 3. Configure Xcode Project

1. **Set Bridging Header:**
   - Project → Target `pollinet-ios` → Build Settings
   - Search: "Objective-C Bridging Header"
   - Set to: `pollinet-ios/pollinet-ios-Bridging-Header.h`

2. **Add Library Search Path:**
   - Build Settings → "Library Search Paths"
   - Add: `$(PROJECT_DIR)/../target/ios`

3. **Link Libraries:**
   - Build Phases → Link Binary With Libraries
   - Add: `libpollinet_device.a` and `libpollinet_sim.a`
   - Or use a script to select based on platform (see full guide)

### 4. Test It!

Update `pollinet_iosApp.swift`:

```swift
import SwiftUI
import SwiftData

@main
struct pollinet_iosApp: App {
    let sdk = PolliNetSDK()
    
    init() {
        // Initialize SDK
        let config = SdkConfig(
            rpcUrl: "https://api.mainnet-beta.solana.com",
            enableLogging: true,
            logLevel: "info"
        )
        
        if sdk.initialize(config: config) {
            print("✅ PolliNet SDK initialized")
            print("Version: \(sdk.getVersion())")
        } else {
            print("❌ Failed to initialize SDK")
        }
    }
    
    var sharedModelContainer: ModelContainer = {
        let schema = Schema([Item.self])
        let modelConfiguration = ModelConfiguration(schema: schema, isStoredInMemoryOnly: false)
        do {
            return try ModelContainer(for: schema, configurations: [modelConfiguration])
        } catch {
            fatalError("Could not create ModelContainer: \(error)")
        }
    }()

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
        .modelContainer(sharedModelContainer)
        .onDisappear {
            sdk.shutdown()
        }
    }
}
```

### 5. Build & Run

- Clean: `Cmd+Shift+K`
- Build: `Cmd+B`
- Run: `Cmd+R`

## 📚 Next Steps

- See `docs/IOS_INTEGRATION_GUIDE.md` for detailed instructions
- Complete the Swift wrapper with all 55 FFI functions
- Add error handling and async/await support

## ⚠️ Troubleshooting

**"Undefined symbols"** → Check library paths and linking
**"Module not found"** → Verify bridging header path
**Build script fails** → Ensure `rustup` and iOS targets are installed
