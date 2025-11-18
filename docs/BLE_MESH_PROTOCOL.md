# PolliNet BLE Mesh Protocol Specification

## Overview

PolliNet uses a BLE mesh network to enable offline Solana transaction broadcasting. Devices form an ad-hoc mesh where transactions propagate peer-to-peer without requiring internet connectivity.

## Network Architecture

### Topology
- **Mesh Type**: Flooding-based with TTL limits
- **Max Hops**: 10 (configurable)
- **Connection Strategy**: Each device maintains 3-8 simultaneous peer connections
- **Role**: All devices act as both relays and endpoints (no dedicated roles)

### Service Discovery
- **Service UUID**: `00001820-0000-1000-8000-00805f9b34fb`
- **Service Name**: `PolliNet`
- **Advertisement**: Connectable, with device capabilities in manufacturer data

## Packet Format

### BLE Characteristics

#### 1. TX Characteristic (Device → Peer)
**UUID**: `00001821-0000-1000-8000-00805f9b34fb`
**Properties**: WRITE, WRITE_NO_RESPONSE
**Purpose**: Send data to connected peer

#### 2. RX Characteristic (Peer → Device)
**UUID**: `00001822-0000-1000-8000-00805f9b34fb`
**Properties**: NOTIFY, READ
**Purpose**: Receive data from connected peer

### Mesh Packet Structure

All packets start with a common header:

```
┌─────────────────────────────────────────────────────────┐
│ Mesh Packet Header (10 bytes)                          │
├──────────┬──────────┬──────────┬──────────┬────────────┤
│  Type    │  Version │   TTL    │  HopCount│  Reserved  │
│ (1 byte) │ (1 byte) │ (1 byte) │ (1 byte) │  (6 bytes) │
└──────────┴──────────┴──────────┴──────────┴────────────┘

┌─────────────────────────────────────────────────────────┐
│ Message ID (16 bytes - UUID)                            │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│ Sender ID (16 bytes - Device UUID)                      │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│ Payload (variable length, max 512 bytes)                │
└─────────────────────────────────────────────────────────┘
```

**Total Header Size**: 42 bytes
**Max Payload**: 512 bytes
**Total Max Packet Size**: 554 bytes

### Packet Types

| Type | Value | Description |
|------|-------|-------------|
| PING | 0x01 | Peer discovery/keepalive |
| PONG | 0x02 | Response to PING |
| TRANSACTION_FRAGMENT | 0x03 | Signed Solana transaction fragment |
| TRANSACTION_ACK | 0x04 | Acknowledge transaction receipt |
| TOPOLOGY_QUERY | 0x05 | Request network topology info |
| TOPOLOGY_RESPONSE | 0x06 | Provide topology information |
| TEXT_MESSAGE | 0x07 | Debug/test text message |

### Transaction Fragment Payload

For `TRANSACTION_FRAGMENT` packets:

```
┌─────────────────────────────────────────────────────────┐
│ Transaction ID (32 bytes - SHA256 of full tx)          │
└─────────────────────────────────────────────────────────┘

┌──────────┬──────────┬──────────┬──────────────────────┐
│ Fragment │  Total   │ Fragment │   Fragment Data      │
│  Index   │Fragments │   Size   │   (variable)         │
│ (2 bytes)│ (2 bytes)│ (2 bytes)│   (up to 400 bytes)  │
└──────────┴──────────┴──────────┴──────────────────────┘
```

**Fragment Overhead**: 38 bytes (mesh header) + 6 bytes (fragment header) = 44 bytes
**Usable Fragment Data**: 512 - 44 = 468 bytes per fragment

## Routing Protocol

### Message Forwarding Rules

1. **Seen Message Cache**: Each device maintains a cache of seen message IDs (last 1000 messages, 10 minute TTL)
2. **TTL Decrement**: Decrease TTL by 1 on each hop; drop if TTL reaches 0
3. **Hop Count Increment**: Increase hop count on each relay
4. **Selective Forwarding**: Only forward to peers who haven't seen the message (based on RSSI and last activity)

### Routing Algorithm (Simplified Flooding)

```rust
fn should_forward_message(
    message_id: &Uuid,
    ttl: u8,
    hop_count: u8,
    sender_peer_id: &str,
    seen_cache: &SeenMessageCache,
) -> bool {
    // Drop if already seen
    if seen_cache.contains(message_id) {
        return false;
    }
    
    // Drop if TTL exhausted
    if ttl == 0 {
        return false;
    }
    
    // Drop if too many hops
    if hop_count >= MAX_HOPS {
        return false;
    }
    
    // Forward to all peers except sender
    true
}
```

### Peer Selection Strategy

When forwarding, prioritize peers by:
1. **Strong RSSI** (> -70 dBm) - closer peers first
2. **Low hop count to other peers** - better connected peers
3. **Recent activity** (seen in last 30 seconds)
4. **Available connection slots** (not at max connections)

## Connection Management

### Connection Limits
- **Min Connections**: 1 (survive with single peer)
- **Target Connections**: 3-5 (balance coverage and overhead)
- **Max Connections**: 8 (Android BLE limit consideration)

### Connection State Machine

```
DISCONNECTED → SCANNING → DISCOVERED → CONNECTING → CONNECTED
     ↑                                                    ↓
     └──────────────────── TIMEOUT/ERROR ←───────────────┘
```

### Reconnection Strategy
- **Immediate Retry**: If peer disconnects, wait 5 seconds, retry once
- **Backoff**: If retry fails, exponential backoff (5s, 10s, 30s, 60s)
- **Peer Rotation**: If peer consistently fails, try different peers

## Fragment Reassembly

### Reassembly Buffer

Each device maintains a buffer of incomplete transactions:

```rust
struct IncompleteTransaction {
    transaction_id: [u8; 32],
    total_fragments: u16,
    received_fragments: HashSet<u16>,
    fragments: HashMap<u16, Vec<u8>>,
    first_seen: Instant,
    last_fragment: Instant,
}
```

### Reassembly Rules

1. **Timeout**: Discard incomplete transactions after 60 seconds
2. **Max Buffer**: Keep max 50 incomplete transactions; evict oldest
3. **Deduplication**: If same fragment received multiple times, keep only one
4. **Completion Check**: Once all fragments received, reconstruct and verify signature

## Network Topology Discovery

### Topology Query
Devices can request topology info from peers:
- **Query Frequency**: Max once per 30 seconds per peer
- **Response**: List of directly connected peers with RSSI

### Network Diameter Estimation
Use topology responses to estimate network size and diameter:
- Helps with TTL tuning
- Provides UX feedback on mesh health

## Security Considerations

### Authentication
- No authentication at BLE mesh layer
- Transactions verified by Solana signature before acceptance

### Spam Prevention
- **Rate Limiting**: Max 10 messages per second per peer
- **Size Limiting**: Max 100 fragments per transaction
- **Memory Limiting**: Max 50 incomplete transactions in buffer

### Privacy
- Device IDs are UUIDs, not tied to Solana addresses
- Transaction contents visible to all mesh participants (transactions are public anyway)

## Performance Targets

| Metric | Target |
|--------|--------|
| Single-hop latency | < 500ms |
| 3-hop latency | < 2 seconds |
| 5-hop latency | < 5 seconds |
| Delivery success rate (3 hops) | > 95% |
| Network discovery time | < 10 seconds |
| Fragment reassembly success | > 99% |

## Future Enhancements

1. **Adaptive TTL**: Adjust TTL based on network density
2. **Directional Routing**: Use RSSI gradients to route toward internet-connected peers
3. **Priority Queuing**: Prioritize small transactions, votes, critical messages
4. **Compression**: LZ4 compression for transaction data
5. **Erasure Coding**: Add redundancy for lossy environments

