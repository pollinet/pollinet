# PolliNet

PolliNet is an open-source SDK and runtime enabling **offline Solana transaction propagation** over Bluetooth Low Energy (BLE) mesh networks. Inspired by biological pollination, transactions are created and signed offline, relayed opportunistically across devices, and submitted to Solana when any peer regains internet access.

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

See the [Whitepaper](https://pollinet.github.io/pollinet/) for detailed technical architecture.

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

