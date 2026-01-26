# How to Fix the OpenSSL Linker Issue for iOS

## The Problem

`solana-sdk` version 2.3.0 with the `"full"` feature pulls in OpenSSL, which tries to link the macOS dylib and fails for iOS.

## The Solution for Solana 2.3.0

Use `solana-sdk` with `default-features = false` and enable ONLY the specific features you need.

## Step 1: Update Cargo.toml

Try this configuration:

```toml
[dependencies]
solana-sdk = { version = "2.3.0", default-features = false }
solana-program = { version = "2.3.0", default-features = false }
```

## Step 2: Check What Features Exist

Run this to see what features `solana-sdk` 2.3.0 offers:

```bash
cargo metadata --format-version 1 | jq '.packages[] | select(.name == "solana-sdk") | .features'
```

Or check the Cargo.toml of solana-sdk 2.3.0 online:
https://docs.rs/crate/solana-sdk/2.3.0/source/Cargo.toml

## Step 3: Enable Only What You Need

Common features that DON'T include OpenSSL:
- `"borsh"` - Borsh serialization
- Maybe individual module features if they exist

## Step 4: Test the Build

```bash
./build-ios.sh
```

If you get errors about missing types (Signature, Keypair, Transaction), you'll need to enable more features.

## Alternative: Upgrade to Solana 3.0+

In Solana 3.0+, the SDK was split into individual crates:
- `solana-signature` 
- `solana-transaction`
- `solana-pubkey`

These individual crates don't have OpenSSL dependencies.

**To upgrade:**
```toml
[dependencies]
solana-signature = "3.0"
solana-transaction = "3.0"
solana-pubkey = "3.0"
```

But this requires updating all Solana dependencies and might break existing code.

## Option 3: Disable OpenSSL Linking (Advanced)

If all else fails, you can try to disable OpenSSL linking at the build level by setting environment variables:

```bash
export OPENSSL_NO_VENDOR=1
export OPENSSL_STATIC=0
./build-ios.sh
```

But this is risky and might not work.

## What to Try Next

1. **Check what's pulling in OpenSSL:**
   ```bash
   cargo tree --target aarch64-apple-ios --no-default-features --features ios -i openssl-sys
   ```

2. **If nothing shows up**, then `solana-sdk` without default features should work!

3. **If something shows up**, trace it back to see which feature enables it

4. **Test the build** and see what compilation errors you get

5. **Enable minimal features** one by one until it compiles

## Expected Result

When configured correctly, you should see:
- ✅ No OpenSSL in dependency tree
- ✅ Signature, Keypair, Transaction types available
- ✅ Successful iOS build

## Current Status

The issue is that `solana-sdk` 2.3.0 doesn't have fine-grained feature flags like later versions. The `"full"` feature is all-or-nothing and includes OpenSSL.

**Your options:**
1. Accept that some heavy dependencies come with `solana-sdk` 2.3.0
2. Upgrade to Solana 3.0+ (individual crates)
3. Create custom re-exports of only the types you need

I recommend **checking if default-features = false is enough**. Try the build and see what happens!
