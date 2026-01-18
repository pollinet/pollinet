```
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│   ██████╗  ██████╗ ██╗     ██╗     ██╗███╗   ██╗███████╗████████╗  
│   ██╔══██╗██╔═══██╗██║     ██║     ██║████╗  ██║██╔════╝╚══██╔══╝  
│   ██████╔╝██║   ██║██║     ██║     ██║██╔██╗ ██║█████╗     ██║     
│   ██╔═══╝ ██║   ██║██║     ██║     ██║██║╚██╗██║██╔══╝     ██║     
│   ██║     ╚██████╔╝███████╗███████╗██║██║ ╚████║███████╗   ██║     
│   ╚═╝      ╚═════╝ ╚══════╝╚══════╝╚═╝╚═╝  ╚═══╝╚══════╝   ╚═╝     
│                                                             │
│            CRITICAL EDGE CASES - 100% COMPLETE              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

Pollinet is an open-source SDK and runtime enabling **offline Solana transaction propagation** over Bluetooth Low Energy (BLE) mesh networks. Inspired by biological pollination, transactions are created and signed offline, relayed opportunistically across devices, and submitted to Solana when any peer regains internet access.

---

## ✨ Features

- **Offline Transaction Relay**  
  Spread signed transactions like pollen grains via BLE mesh.

- **Nonce Account Support**  
  Extend transaction lifespan beyond recent blockhash constraints.

- **LZ4 Compression**  
  Efficient lossless compression of transaction payloads.

- **Fragmentation & Reassembly**  
  Split large transactions to fit BLE packet size limits.

- **Store & Forward**  
  Cache transactions when no gateway is available.

- **Confirmation Routing**  
  Deliver submission confirmations back to origin devices.

## 🧭 Platform Support

- **Android (Production)** – Foreground BLE service, GATT bridge, and diagnostics UI. This is the path we ship and support for real-world mesh relays.
- **Desktop Simulation (Linux/macOS)** – The Rust examples and Linux BLE adapter are kept for local debugging, CI smoke tests, and mesh simulations only. They are not hardened for production deployments.

---

## 📚 Documentation

- **[Whitepaper](https://pollinet.github.io/pollinet/)** – Detailed technical architecture
- **[Offline Transactions Guide](./docs/OFFLINE_TRANSACTIONS_GUIDE.md)** – Complete guide to offline transactions and nonce accounts
- **[Testing Guide](./TESTING.md)** – Comprehensive testing documentation
- **[Publishing Guide](./docs/PUBLISHING_GUIDE.md)** – How to publish Kotlin SDK to Maven Central or GitHub Packages
- **[Scripts Reference](./scripts/README.md)** – Utility scripts documentation

---

## 🚀 Getting Started

### Prerequisites

- Rust toolchain (`cargo`, `rustc`)
- Bluetooth LE-compatible device
- Solana CLI installed for account funding and nonce management

---

### Building the Project

```bash
cargo build --release
```

### Quick Start: Running Examples

PolliNet uses **nonce accounts** to enable offline transactions. Each nonce account allows exactly **one offline transaction** before requiring an internet refresh.

**Key Concept:** 
- **Number of nonce accounts = Number of offline transactions you can perform**
- Nonces are cached and reused to minimize costs (~99% savings!)

**Quick Start:**
```bash
# Prepare nonce bundle (REQUIRED FIRST)
./scripts/pollinet_cli.sh prepare

# Run examples
cargo run --example offline_transaction_flow

# Refresh nonces after use
./scripts/pollinet_cli.sh refresh-nonces
```

📖 **For detailed instructions, examples, and workflows, see [OFFLINE_TRANSACTIONS_GUIDE.md](./OFFLINE_TRANSACTIONS_GUIDE.md)**

---

## 🧪 Testing

Run the comprehensive test suite:

```bash
./scripts/test_pollinet.sh          # All tests (includes M1 demo)
./scripts/test_pollinet.sh --quick  # Quick tests (skip M1 demo)
./scripts/test_pollinet.sh --m1-only # M1 demo only (50+ transactions)
```

📖 **For detailed testing information, see [TESTING.md](./TESTING.md)**

---

