# OpenSSL Linker Issue for iOS

## The Problem

The linker error shows:
```
ld: building for 'iOS', but linking in dylib (/opt/homebrew/Cellar/openssl@3/3.6.0/lib/libssl.3.dylib) built for 'macOS'
```

This happens because:
1. We enabled `solana-sdk` with `features = ["full"]`
2. The `"full"` feature enables many sub-crates including `solana-precompiles`
3. `solana-precompiles` includes `solana-secp256r1-program`
4. `solana-secp256r1-program` depends on `openssl` crate
5. The `openssl` crate tries to link system OpenSSL
6. On your Mac, it finds the Homebrew macOS OpenSSL, which can't link to iOS

## Why This is a Problem

- iOS builds can't link macOS dylibs
- OpenSSL is not needed for iOS (we excluded RPC client)
- We only need the basic Solana types (Signature, Keypair, Transaction, Pubkey)

## The Solution

**Don't use the `"full"` feature for iOS.** Instead, we need to:

1. Use `solana-sdk` without default features
2. Only enable the specific sub-features we need
3. Make sure those sub-features don't pull in OpenSSL

## What We Actually Need

From our code analysis:
- `solana_sdk::pubkey::Pubkey` - for public keys
- `solana_sdk::signature::Signature` - for signatures
- `solana_sdk::signature::Keypair` - for keypairs  
- `solana_sdk::transaction::Transaction` - for transactions (from `TransactionService`)

## Investigation Needed

Check which minimal features of `solana-sdk` provide these types without OpenSSL.

Options:
1. Use individual solana crates (`solana-signature`, `solana-keypair`, `solana-transaction`, `solana-pubkey`)
2. Find the minimal `solana-sdk` feature set that doesn't include `openssl`

## Next Steps

1. Remove `features = ["full"]` from `solana-sdk`
2. Add individual Solana crates as dependencies
3. Update imports in code to use individual crates
4. Test iOS build

