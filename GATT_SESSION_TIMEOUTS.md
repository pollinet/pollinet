# GATT Session Timeout Configuration Guide

## Overview
This document explains how to configure GATT session timeouts in the PolliNet BLE mesh simulation. GATT sessions control how long Bluetooth Low Energy connections remain active and how the system handles data exchange.

## Current Timeout Settings

### **Default Configuration:**
- **Data Receive Timeout:** 30 seconds
- **Session Keepalive:** 60 seconds  
- **Scan Interval:** 5 seconds
- **Max Concurrent Sessions:** 5

### **Extended Timeouts Configuration:**
- **Data Receive Timeout:** 120 seconds (2 minutes)
- **Session Keepalive:** 300 seconds (5 minutes)
- **Scan Interval:** 10 seconds
- **Max Concurrent Sessions:** 10

## Configuration Options

### **1. Quick Configuration Changes**

#### **Option A: Modify Constants**
```rust
// In examples/ble_mesh_simulation.rs
const GATT_DATA_RECEIVE_TIMEOUT_SECS: u64 = 60;  // Increase to 60 seconds
const GATT_SESSION_KEEPALIVE_SECS: u64 = 120;    // Increase to 2 minutes
const SCAN_INTERVAL_SECS: u64 = 10;              // Increase to 10 seconds
```

#### **Option B: Use Predefined Configurations**
```rust
// In main() function, change this line:
let gatt_config = GattSessionConfig::with_extended_timeouts();

// Available options:
// - GattSessionConfig::default() - Standard timeouts
// - GattSessionConfig::with_extended_timeouts() - Longer timeouts
// - GattSessionConfig::battery_optimized() - Shorter timeouts for battery
// - GattSessionConfig::high_performance() - Faster scanning
```

### **2. Custom Configuration**
```rust
let custom_config = GattSessionConfig {
    data_receive_timeout_secs: 180,  // 3 minutes
    session_keepalive_secs: 600,     // 10 minutes
    scan_interval_secs: 15,          // 15 seconds
    max_concurrent_sessions: 15,
};
GATT_CONFIG.set(custom_config).unwrap();
```

## Timeout Types Explained

### **1. Data Receive Timeout**
- **What it does:** How long to wait for incoming data from a connected device
- **When it's used:** During scanning mode when connecting to discovered devices
- **Default:** 30 seconds
- **Increase when:** 
  - Devices are slow to respond
  - Large data transfers are expected
  - Network conditions are poor
- **Decrease when:**
  - Fast response times are needed
  - Battery life is critical
  - Quick connection cycling is desired

### **2. Session Keepalive**
- **What it does:** How long to keep GATT session alive after data exchange
- **When it's used:** After successful data exchange to maintain connection
- **Default:** 60 seconds
- **Increase when:**
  - Persistent connections are needed
  - Multiple data exchanges are expected
  - Connection setup is expensive
- **Decrease when:**
  - Resources need to be freed quickly
  - Battery life is critical
  - One-time data exchange is sufficient

### **3. Scan Interval**
- **What it does:** How often to perform BLE scans for new devices
- **When it's used:** Continuously during mesh operation
- **Default:** 5 seconds
- **Increase when:**
  - Battery life is critical
  - Fewer devices are expected
  - Less frequent discovery is acceptable
- **Decrease when:**
  - Fast device discovery is needed
  - High-frequency updates are required
  - More devices are expected

### **4. Max Concurrent Sessions**
- **What it does:** Maximum number of simultaneous GATT connections
- **When it's used:** When establishing new connections
- **Default:** 5
- **Increase when:**
  - Many devices need to be connected simultaneously
  - High throughput is required
  - System has sufficient resources
- **Decrease when:**
  - System resources are limited
  - Battery life is critical
  - Fewer connections are needed

## Predefined Configurations

### **Standard (Default)**
```rust
GattSessionConfig::default()
```
- Data receive: 30 seconds
- Session keepalive: 60 seconds
- Scan interval: 5 seconds
- Max sessions: 5
- **Best for:** General use, balanced performance

### **Extended Timeouts**
```rust
GattSessionConfig::with_extended_timeouts()
```
- Data receive: 120 seconds (2 minutes)
- Session keepalive: 300 seconds (5 minutes)
- Scan interval: 10 seconds
- Max sessions: 10
- **Best for:** Slow networks, large data transfers, persistent connections

### **Battery Optimized**
```rust
GattSessionConfig::battery_optimized()
```
- Data receive: 15 seconds
- Session keepalive: 30 seconds
- Scan interval: 30 seconds
- Max sessions: 3
- **Best for:** Battery-powered devices, infrequent communication

### **High Performance**
```rust
GattSessionConfig::high_performance()
```
- Data receive: 60 seconds
- Session keepalive: 180 seconds (3 minutes)
- Scan interval: 2 seconds
- Max sessions: 20
- **Best for:** High-throughput scenarios, many devices, fast discovery

## How to Change Configuration

### **Step 1: Choose Your Configuration**
```rust
// In main() function, replace this line:
let gatt_config = GattSessionConfig::with_extended_timeouts();

// With one of these:
let gatt_config = GattSessionConfig::default();                    // Standard
let gatt_config = GattSessionConfig::with_extended_timeouts();     // Extended
let gatt_config = GattSessionConfig::battery_optimized();          // Battery
let gatt_config = GattSessionConfig::high_performance();           // High performance
```

### **Step 2: Compile and Run**
```bash
cargo run --example ble_mesh_simulation
```

### **Step 3: Verify Configuration**
The system will log the configuration on startup:
```
✅ GATT session configuration initialized:
   📡 Data receive timeout: 120 seconds
   🔗 Session keepalive: 300 seconds
   🔍 Scan interval: 10 seconds
   👥 Max concurrent sessions: 10
```

## Troubleshooting

### **Connection Timeouts**
- **Problem:** Devices disconnect before data exchange completes
- **Solution:** Increase `data_receive_timeout_secs`
- **Example:** Set to 60-120 seconds for slow devices

### **Battery Drain**
- **Problem:** System consumes too much battery
- **Solution:** Use `battery_optimized()` configuration
- **Example:** Increase scan interval to 30+ seconds

### **Slow Discovery**
- **Problem:** Takes too long to find new devices
- **Solution:** Decrease `scan_interval_secs`
- **Example:** Set to 2-3 seconds for faster discovery

### **Resource Exhaustion**
- **Problem:** Too many concurrent connections
- **Solution:** Decrease `max_concurrent_sessions`
- **Example:** Set to 3-5 for limited resources

## Best Practices

1. **Start with default configuration** and adjust based on your needs
2. **Monitor logs** to see actual timeout behavior
3. **Test with your specific devices** to find optimal settings
4. **Balance performance vs battery life** based on your use case
5. **Consider network conditions** when setting timeouts
6. **Use extended timeouts** for unreliable networks
7. **Use battery optimized** for mobile/portable devices

## Example Scenarios

### **Scenario 1: IoT Sensor Network**
- **Need:** Reliable data collection from many sensors
- **Configuration:** `with_extended_timeouts()`
- **Reason:** Sensors may be slow, need persistent connections

### **Scenario 2: Mobile App**
- **Need:** Quick device discovery and connection
- **Configuration:** `high_performance()`
- **Reason:** Fast user experience, many devices

### **Scenario 3: Battery-Powered Tracker**
- **Need:** Long battery life with occasional communication
- **Configuration:** `battery_optimized()`
- **Reason:** Minimize power consumption

### **Scenario 4: Development/Testing**
- **Need:** Fast iteration and debugging
- **Configuration:** `default()` with custom adjustments
- **Reason:** Balanced performance for development
