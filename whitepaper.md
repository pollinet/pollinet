# PolliNet Technical Whitepaper

## Abstract

PolliNet is a decentralized SDK and runtime enabling Solana transactions to be distributed opportunistically over Bluetooth Low Energy (BLE) mesh networks. Inspired by biological pollination, transactions (“pollen grains”) are created offline, propagated across peer devices, and eventually submitted to the Solana blockchain by any gateway node with internet connectivity. PolliNet provides lossless compression (LZ4), store-and-forward caching, and robust fragmentation, allowing transactions to spread efficiently and reliably, even under severe network constraints.

---

## Table of Contents

1. Introduction
2. System Overview
3. Bluetooth Mesh Network
4. Transaction Distribution Protocol
5. Nonce Account Management
6. Compression and Fragmentation
7. Security Model
8. SDK Architecture
9. Future Extensions
10. Conclusion

---

## Introduction

Traditional Solana transactions require constant internet connectivity, which limits adoption in rural areas, disaster scenarios, and censorship-prone environments. PolliNet addresses this limitation by introducing a Bluetooth mesh-based relay system that distributes signed transactions like pollen grains across devices until one with internet connectivity submits them to the blockchain.

Key benefits:

- **Decentralized**: No dependency on centralized infrastructure.
- **Resilient**: Works in fully offline settings with eventual consistency.
- **Secure**: Transactions are signed in advance, ensuring authenticity.
- **Efficient**: LZ4 compression and fragmentation reduce overhead.
- **Extensible**: SDK can integrate with any Solana-based wallet or app.

---

## System Overview

PolliNet operates in three phases:

1. **Creation**: A device creates and signs a Solana transaction using a nonce account.
2. **Propagation**: The signed transaction is serialized, compressed, and relayed over BLE to nearby peers.
3. **Submission**: When any peer with internet connectivity receives the transaction, it submits it to a Solana RPC endpoint and broadcasts the confirmation back through the mesh.

This process mimics the way pollen grains disperse via wind or pollinators, reaching the destination in a decentralized manner.

---

## Bluetooth Mesh Network

PolliNet uses a BLE mesh similar to peer-to-peer messaging apps:

- **Advertise**: Devices broadcast their presence and capabilities (e.g., "CAN_SUBMIT_SOLANA").
- **Scan**: Devices look for peers advertising the same service UUID.
- **Connect**: Peers establish BLE connections as both Central and Peripheral.
- **Relay**: Devices forward or cache received transactions.

**Network Topology:**

- **Clusters**: Local groups of devices within ~30 meters.
- **Bridges**: Nodes connecting clusters when they come into range.
- **Store-and-Forward**: If no internet is available, transactions are cached locally.

---

## Transaction Distribution Protocol

Each transaction is distributed as a binary payload:

- **Payload Fields**:
  - Serialized Solana transaction (using `solana-sdk`)
  - Metadata (e.g., max fee, expiration)
  - Compression flag
  - Fragmentation index

**TTL-Based Routing**:
- Each packet includes a Time-To-Live counter (default = 7 hops).
- Devices decrement TTL on relay.
- TTL=0 packets are discarded to prevent loops.

**Reliability**:
- Duplicate message detection via unique transaction IDs.
- Opportunistic multi-gateway submission to increase success rates.

---

## Nonce Account Management

To extend transaction lifespan beyond recent blockhash limits, PolliNet relies on Solana nonce accounts:

- **Creation**:
  - Funded with a small SOL balance.
  - Created once and reused until exhausted.
- **AdvanceNonceAccount Instruction**:
  - Always the first instruction.
  - Ensures that each relay remains valid until submission.
- **Offline Signing**:
  - Transactions are signed *before* propagation.
  - Gateways cannot modify or forge them.

---

## Compression and Fragmentation

**Compression**:
- Transactions are compressed using **LZ4**, providing:
  - ~30–70% size reduction for typical Solana transactions.
  - Fast encoding/decoding suitable for mobile devices.
- Compression applied if payload >100 bytes.

**Fragmentation**:
- BLE MTU limits packets (~500 bytes).
- Large messages are split into:
  - `FRAGMENT_START`
  - `FRAGMENT_CONTINUE`
  - `FRAGMENT_END`
- Receiving peers reassemble fragments automatically.

---

## Security Model

PolliNet is designed to be secure by default:

- **End-to-End Integrity**:
  - Transactions are pre-signed, preventing tampering.
- **No Private Keys in Transit**:
  - Only signed transaction blobs are relayed.
- **Replay Protection**:
  - Nonce accounts prevent duplication.
- **Confirmation Signatures**:
  - Gateways return Solana transaction signatures to prove submission.
- **Optional Encryption**:
  - Future versions may encrypt payloads to conceal metadata.

---

## SDK Architecture

**Core Components**:

1. **TransactionBuilder**
   - Creates and signs nonce transactions.
2. **MeshTransport**
   - Handles BLE scanning, advertising, and relay.
3. **CompressionService**
   - LZ4 compress/decompress logic.
4. **FragmentHandler**
   - Splits and reassembles messages.
5. **SubmissionService**
   - Submits transactions to Solana RPC.
6. **ConfirmationRouter**
   - Routes submission confirmations back to the origin.

**Languages**:
- Rust (core reference)
- Swift (iOS)
- Kotlin (Android)
- JavaScript/TypeScript (React Native)

---

## Future Extensions

Potential future improvements:

- **WiFi Direct Transport**:
  - Higher bandwidth, longer range.
- **LoRa Integration**:
  - Extreme-range mesh relays.
- **Cross-Chain Support**:
  - Distributing transactions for other blockchains.
- **Incentive Mechanisms**:
  - Rewards for acting as a gateway.

---

## Conclusion

PolliNet enables resilient, decentralized transaction submission for Solana, inspired by the natural process of pollination. By combining BLE mesh networking, nonce accounts, LZ4 compression, and opportunistic gateways, it expands the blockchain’s reach to any environment—online or offline.

---

*This whitepaper is released under the Apache 2.0 License.*
