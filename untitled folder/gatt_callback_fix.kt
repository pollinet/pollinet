// Add this inside your gattCallback object (around line 563 in your code)
// This goes AFTER onMtuChanged() and BEFORE onCharacteristicChanged()

@SuppressLint("MissingPermission")
override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
    appendLog("📋 Services discovered: status=$status")
    
    if (status != BluetoothGatt.GATT_SUCCESS) {
        appendLog("❌ Service discovery failed with status: $status")
        return
    }
    
    // Log all discovered services and characteristics
    appendLog("🔍 === DISCOVERED SERVICES & CHARACTERISTICS ===")
    gatt.services.forEach { service ->
        appendLog("📦 Service: ${service.uuid}")
        appendLog("   Type: ${if (service.type == BluetoothGattService.SERVICE_TYPE_PRIMARY) "PRIMARY" else "SECONDARY"}")
        
        service.characteristics.forEach { characteristic ->
            appendLog("   📝 Characteristic: ${characteristic.uuid}")
            
            // Log properties
            val properties = mutableListOf<String>()
            if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_READ != 0) {
                properties.add("READ")
            }
            if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_WRITE != 0) {
                properties.add("WRITE")
            }
            if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE != 0) {
                properties.add("WRITE_NO_RESPONSE")
            }
            if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_NOTIFY != 0) {
                properties.add("NOTIFY")
            }
            if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_INDICATE != 0) {
                properties.add("INDICATE")
            }
            appendLog("      Properties: ${properties.joinToString(", ")}")
            
            // Log descriptors
            characteristic.descriptors.forEach { descriptor ->
                appendLog("      🔖 Descriptor: ${descriptor.uuid}")
            }
        }
    }
    appendLog("🔍 === END OF DISCOVERED SERVICES ===")
    
    // Find our PolliNet service
    val service = gatt.getService(SERVICE_UUID)
    if (service == null) {
        appendLog("⚠️ PolliNet service not found!")
        appendLog("   Expected: $SERVICE_UUID")
        appendLog("   Available services: ${gatt.services.map { it.uuid }}")
        return
    }
    
    appendLog("✅ PolliNet service found: $SERVICE_UUID")
    
    // Get our characteristics
    remoteTxCharacteristic = service.getCharacteristic(TX_CHAR_UUID)
    remoteRxCharacteristic = service.getCharacteristic(RX_CHAR_UUID)
    
    if (remoteTxCharacteristic == null || remoteRxCharacteristic == null) {
        appendLog("❌ Missing PolliNet characteristics!")
        appendLog("   TX characteristic ${if (remoteTxCharacteristic != null) "✅" else "❌"}: $TX_CHAR_UUID")
        appendLog("   RX characteristic ${if (remoteRxCharacteristic != null) "✅" else "❌"}: $RX_CHAR_UUID")
        return
    }
    
    appendLog("✅ Characteristics ready:")
    appendLog("   TX (notify): $TX_CHAR_UUID")
    appendLog("   RX (write): $RX_CHAR_UUID")
    
    // Check bonding state before attempting descriptor write
    val bondState = gatt.device.bondState
    appendLog("🔐 Device bond state: ${bondState.toBondStateString()}")
    
    // Enable notifications on TX characteristic
    val notifySuccess = gatt.setCharacteristicNotification(remoteTxCharacteristic, true)
    appendLog("📬 setCharacteristicNotification: $notifySuccess")
    
    // Write CCCD to enable remote notifications
    val descriptor = remoteTxCharacteristic?.getDescriptor(cccdUuid)
    if (descriptor == null) {
        appendLog("❌ CCCD descriptor not found!")
        appendLog("   Cannot receive notifications without CCCD")
        return
    }
    
    appendLog("✅ CCCD descriptor found: $cccdUuid")
    
    // If not bonded, try to bond first (some devices require bonding for descriptor writes)
    if (bondState == BluetoothDevice.BOND_NONE) {
        appendLog("🔐 Device not bonded - creating bond before descriptor write...")
        try {
            gatt.device.createBond()
            // Store descriptor for retry after bonding
            pendingDescriptorWrite = descriptor
            pendingGatt = gatt
            return
        } catch (e: Exception) {
            appendLog("❌ Failed to create bond: ${e.message}")
            // Fall through and try descriptor write anyway
        }
    }
    
    // Write descriptor to enable notifications
    descriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
    val writeSuccess = gatt.writeDescriptor(descriptor)
    appendLog("📬 Writing CCCD descriptor to enable notifications: $writeSuccess")
    
    if (!writeSuccess) {
        appendLog("⚠️ Descriptor write queuing failed!")
        appendLog("   This may indicate the GATT queue is full or device is busy")
    } else {
        appendLog("⏳ Waiting for onDescriptorWrite callback...")
        appendLog("   Data transfer will begin after descriptor write confirms")
    }
}
