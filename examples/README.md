# PolliNet Examples

This directory contains examples demonstrating PolliNet SDK functionality.

## Transaction Flow Test

**File:** `transaction_flow_test.rs`  
**Documentation:** `TRANSACTION_FLOW_DEMO.md`

Demonstrates the complete transaction lifecycle through PolliNet's BLE mesh network:
1. Fragment transaction
2. Queue to outbound
3. Transmit over BLE
4. Receive fragments
5. Reassemble transaction
6. Queue to received
7. Submit to Solana RPC
8. Queue confirmation

### Platform Compatibility

- ✅ **Linux** - Can run the example with `dbus` installed
- ✅ **Android** - Full implementation in `pollinet-android/`
- ❌ **macOS** - Cannot run due to BLE library requiring `dbus`

### Running on macOS

If you're on macOS, you have two options:

1. **Read the code and documentation** (recommended):
   - Review `transaction_flow_test.rs` for implementation details
   - Read `TRANSACTION_FLOW_DEMO.md` for step-by-step walkthrough
   - See expected output and behavior

2. **Test on Android devices**:
   ```bash
   cd pollinet-android
   ./gradlew installDebug
   ```
   - Open the app on two devices
   - Use the "MWA Transaction Demo" screen
   - Test the complete sender → receiver flow

### Running on Linux

```bash
# Install dependencies
sudo apt install libdbus-1-dev pkg-config

# Run the example
cargo run --example transaction_flow_test
```

## Other Examples

See the main repository examples for:
- Offline transaction creation
- Nonce account management  
- MWA integration
- SPL token transfers

Located in the root `examples/` directory.

