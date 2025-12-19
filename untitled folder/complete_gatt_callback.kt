// COMPLETE gattCallback - Replace your existing gattCallback (around line 408-563)

private val gattCallback = object : BluetoothGattCallback() {
    
    @SuppressLint("MissingPermission")
    override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
        appendLog("🔄 Connection state change: status=$status, newState=$newState")
        
        // Handle error statuses
        if (status != BluetoothGatt.GATT_SUCCESS) {
            appendLog("❌ Connection error: status=$status")
            when (status) {
                5, 15 -> {
                    appendLog("🔐 Authentication/Encryption required - creating bond...")
                    try {
                        gatt.device.createBond()
                    } catch (e: Exception) {
                        appendLog("❌ Failed to create bond: ${e.message}")
                    }
                }
                22 -> {
                    appendLog("🔐 GATT_INSUFFICIENT_AUTHORIZATION (22) – NOT auto-bonding, just logging")
                }
                133 -> {
                    appendLog("⚠️ GATT_ERROR - refreshing cache and retrying...")
                    refreshDeviceCache(gatt)
                    gatt.close()
                    clientGatt = null
                }
                else -> {
                    appendLog("❌ Error: See https://developer.android.com/reference/android/bluetooth/BluetoothGatt")
                }
            }
            _connectionState.value = ConnectionState.DISCONNECTED
            return
        }
        
        // Handle connection states
        when (newState) {
            BluetoothProfile.STATE_CONNECTED -> {
                _connectionState.value = ConnectionState.CONNECTED
                connectedDevice = gatt.device
                clientGatt = gatt
                appendLog("✅ Connected to ${gatt.device.address}")
                
                // Request MTU for better throughput
                appendLog("📏 Requesting MTU negotiation (target: 247 bytes)...")
                appendLog("   Current default: $currentMtu bytes")
                val mtuRequested = gatt.requestMtu(247)
                if (!mtuRequested) {
                    appendLog("⚠️ MTU request failed, using default: $currentMtu")
                }
                
                // Request high connection priority
                val priorityResult = gatt.requestConnectionPriority(BluetoothGatt.CONNECTION_PRIORITY_HIGH)
                appendLog("⚡ Connection priority: HIGH (result=$priorityResult, ~7.5ms interval)")
                
                // Service discovery happens in onMtuChanged
            }
            BluetoothProfile.STATE_DISCONNECTED -> {
                _connectionState.value = ConnectionState.DISCONNECTED
                appendLog("🔌 Disconnected from ${gatt.device.address}")
                
                // Clean up
                connectedDevice = null
                clientGatt = null
                remoteTxCharacteristic = null
                remoteRxCharacteristic = null
                remoteWriteInProgress = false
                operationInProgress = false
                operationQueue.clear()
                descriptorWriteRetries = 0
                pendingDescriptorWrite = null
                pendingGatt = null
                sendingJob?.cancel()
                
                // Clear re-fragmentation tracking
                fragmentsQueuedWithMtu = 0
                
                // Reset descriptor write flag
                descriptorWriteComplete = false
            }
        }
    }
    
    override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
        val oldMtu = currentMtu
        currentMtu = mtu
        val maxPayload = (mtu - 10).coerceAtLeast(20)
        val oldMaxPayload = (oldMtu - 10).coerceAtLeast(20)
        appendLog("📏 MTU negotiation complete: $oldMtu → $mtu bytes (status=$status)")
        appendLog("   Max payload per fragment: $maxPayload bytes")
        appendLog("   Expected fragments for 1KB tx: ~${1024 / maxPayload} (was ~${1024 / oldMaxPayload})")
        
        // Re-queue fragments with new MTU if significantly larger
        val mtuIncrease = mtu - oldMtu
        if (mtuIncrease >= 30 && pendingTransactionBytes != null) {
            appendLog("🔄 MTU increased by $mtuIncrease bytes - re-fragmenting with larger size...")
            appendLog("   Pausing sending loop for re-fragmentation...")
            
            sendingJob?.cancel()
            
            serviceScope.launch {
                val txBytes = pendingTransactionBytes
                if (txBytes != null) {
                    val sdkInstance = sdk
                    if (sdkInstance != null) {
                        appendLog("♻️ Re-fragmenting ${txBytes.size} bytes with new MTU...")
                        val newMaxPayload = (currentMtu - 10).coerceAtLeast(20)
                        sdkInstance.fragment(txBytes, newMaxPayload).onSuccess { fragments ->
                            val newCount = fragments.fragments.size
                            val oldCount = (txBytes.size + oldMaxPayload - 1) / oldMaxPayload
                            appendLog("✅ Re-fragmented: $oldCount → $newCount fragments")
                            appendLog("   Improvement: ${((oldCount - newCount).toFloat() / oldCount * 100).toInt()}% fewer fragments")
                            
                            fragmentsQueuedWithMtu = currentMtu
                            ensureSendingLoopStarted()
                        }.onFailure {
                            appendLog("❌ Re-fragmentation failed: ${it.message}")
                            ensureSendingLoopStarted()
                        }
                    } else {
                        appendLog("⚠️ SDK not available for re-fragmentation")
                    }
                }
            }
        } else if (mtuIncrease < 30) {
            appendLog("   MTU increase too small ($mtuIncrease bytes), keeping existing fragments")
        }
        
        // CRITICAL: Discover services after MTU negotiation
        appendLog("🔍 Starting service discovery...")
        val discoverSuccess = gatt.discoverServices()
        if (!discoverSuccess) {
            appendLog("❌ Failed to start service discovery!")
        }
    }

    // ============================================
    // NEW: onServicesDiscovered implementation
    // ============================================
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
        
        // Check bonding state
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
        
        // If not bonded, try to bond first
        if (bondState == BluetoothDevice.BOND_NONE) {
            appendLog("🔐 Device not bonded - creating bond before descriptor write...")
            try {
                gatt.device.createBond()
                pendingDescriptorWrite = descriptor
                pendingGatt = gatt
                return
            } catch (e: Exception) {
                appendLog("❌ Failed to create bond: ${e.message}")
            }
        }
        
        // Write descriptor to enable notifications
        descriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
        val writeSuccess = gatt.writeDescriptor(descriptor)
        appendLog("📬 Writing CCCD descriptor to enable notifications: $writeSuccess")
        
        if (!writeSuccess) {
            appendLog("⚠️ Descriptor write queuing failed!")
        } else {
            appendLog("⏳ Waiting for onDescriptorWrite callback...")
            appendLog("   Data transfer will begin after descriptor write confirms")
        }
    }

    override fun onCharacteristicChanged(
        gatt: BluetoothGatt,
        characteristic: BluetoothGattCharacteristic,
        value: ByteArray
    ) {
        serviceScope.launch {
            if (sdk == null) {
                appendLog("⚠️ SDK not initialized; inbound dropped")
                return@launch
            }
            appendLog("⬅️ Received: ${previewFragment(value)}")
            handleReceivedData(value)
        }
    }

    override fun onCharacteristicWrite(
        gatt: BluetoothGatt,
        characteristic: BluetoothGattCharacteristic,
        status: Int
    ) {
        if (characteristic.uuid == RX_CHAR_UUID) {
            operationInProgress = false
            
            if (status == BluetoothGatt.GATT_SUCCESS) {
                completeRemoteWrite()
                processOperationQueue()
            } else {
                remoteWriteInProgress = false
                appendLog("❌ Write failed with status $status")
                
                if (status == 133) {
                    handleStatus133(gatt)
                } else {
                    processOperationQueue()
                }
            }
        }
    }

    @SuppressLint("MissingPermission")
    override fun onDescriptorWrite(
        gatt: BluetoothGatt,
        descriptor: BluetoothGattDescriptor,
        status: Int
    ) {
        appendLog("📝 Descriptor write: status=$status")
        
        if (status == BluetoothGatt.GATT_SUCCESS) {
            appendLog("✅ Notifications enabled - ready to transfer data!")
            descriptorWriteRetries = 0
            pendingDescriptorWrite = null
            pendingGatt = null
            
            // Mark descriptor write as complete
            descriptorWriteComplete = true
            
            // NOW we can start sending data
            ensureSendingLoopStarted()
        } else {
            appendLog("❌ Failed to enable notifications: status=$status")
            
            if (status == 133) {
                sendingJob?.cancel()
                appendLog("⚠️ Status 133 detected - pausing sending loop for recovery")
                
                if (descriptorWriteRetries < MAX_DESCRIPTOR_RETRIES) {
                    descriptorWriteRetries++
                    appendLog("⚠️ Retrying descriptor write (attempt $descriptorWriteRetries/$MAX_DESCRIPTOR_RETRIES)...")
                    
                    refreshDeviceCache(gatt)
                    
                    val retryDelay = 1000L * descriptorWriteRetries
                    mainHandler.postDelayed({
                        try {
                            gatt.setCharacteristicNotification(remoteTxCharacteristic, true)
                            val retryDescriptor = remoteTxCharacteristic?.getDescriptor(cccdUuid)
                            if (retryDescriptor != null) {
                                retryDescriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                                pendingDescriptorWrite = retryDescriptor
                                pendingGatt = gatt
                                gatt.writeDescriptor(retryDescriptor)
                                appendLog("🔄 Retrying descriptor write...")
                            } else {
                                appendLog("❌ CCCD descriptor not found for retry")
                            }
                        } catch (e: Exception) {
                            appendLog("❌ Retry failed: ${e.message}")
                            descriptorWriteRetries = 0
                        }
                    }, retryDelay)
                } else {
                    appendLog("❌ Max descriptor write retries reached. Giving up.")
                    descriptorWriteRetries = 0
                    handleStatus133(gatt)
                }
            } else if (status == 5 || status == 15) {
                appendLog("🔐 Bonding required for descriptor write - creating bond...")
                try {
                    gatt.device.createBond()
                    pendingDescriptorWrite = descriptor
                    pendingGatt = gatt
                } catch (e: Exception) {
                    appendLog("❌ Failed to create bond: ${e.message}")
                }
            } else if (status == 22) {
                appendLog("🔐 GATT_INSUFFICIENT_AUTHORIZATION (22) – NOT auto-bonding, just logging")
            } else {
                descriptorWriteRetries = 0
            }
        }
    }
}
