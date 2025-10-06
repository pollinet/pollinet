//! GATT service specification for PolliNet
//! 
//! Defines the BLE GATT service structure for cross-platform implementation

use uuid::Uuid;
use serde::{Deserialize, Serialize};

/// PolliNet GATT Service UUID (fixed)
pub const POLLINET_SERVICE_UUID: &str = "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a7";

/// Characteristic UUIDs
pub const FRAGMENT_TX_CHAR_UUID: &str = "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a8";
pub const FRAGMENT_RX_CHAR_UUID: &str = "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a9";
pub const DEVICE_INFO_CHAR_UUID: &str = "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12aa";
pub const CONFIRMATION_CHAR_UUID: &str = "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12ab";
pub const STATUS_CHAR_UUID: &str = "7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12ac";

/// GATT characteristic properties
#[derive(Debug, Clone, Copy)]
pub enum CharacteristicProperty {
    Read = 0x02,
    Write = 0x08,
    WriteWithoutResponse = 0x04,
    Notify = 0x10,
    Indicate = 0x20,
}

/// PolliNet GATT Service definition
#[derive(Debug, Clone)]
pub struct PolliNetGattService {
    /// Service UUID
    pub service_uuid: Uuid,
    
    /// Characteristics
    pub characteristics: Vec<GattCharacteristic>,
}

/// GATT Characteristic definition
#[derive(Debug, Clone)]
pub struct GattCharacteristic {
    /// Characteristic UUID
    pub uuid: Uuid,
    
    /// Characteristic properties (bitmask)
    pub properties: u8,
    
    /// Maximum value length
    pub max_length: usize,
    
    /// Current value
    pub value: Vec<u8>,
    
    /// Descriptors
    pub descriptors: Vec<GattDescriptor>,
}

/// GATT Descriptor definition
#[derive(Debug, Clone)]
pub struct GattDescriptor {
    /// Descriptor UUID
    pub uuid: Uuid,
    
    /// Descriptor value
    pub value: Vec<u8>,
}

/// Device information for advertising
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Device identifier
    pub device_id: String,
    
    /// Device capabilities
    pub capabilities: Vec<DeviceCapability>,
    
    /// Protocol version
    pub protocol_version: String,
    
    /// Maximum MTU supported
    pub max_mtu: u16,
}

/// Device capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceCapability {
    CanRelay,
    CanSubmit,
    HasInternet,
    SupportsCompression,
}

impl PolliNetGattService {
    /// Create the standard PolliNet GATT service definition
    pub fn create_standard_service() -> Self {
        let service_uuid = Uuid::parse_str(POLLINET_SERVICE_UUID)
            .expect("Invalid service UUID");
        
        let mut characteristics = Vec::new();
        
        // 1. Fragment TX Characteristic (Write, WriteWithoutResponse)
        characteristics.push(GattCharacteristic {
            uuid: Uuid::parse_str(FRAGMENT_TX_CHAR_UUID).unwrap(),
            properties: CharacteristicProperty::Write as u8 
                | CharacteristicProperty::WriteWithoutResponse as u8,
            max_length: 512, // Maximum BLE packet size
            value: Vec::new(),
            descriptors: vec![
                GattDescriptor {
                    uuid: Uuid::parse_str("00002901-0000-1000-8000-00805f9b34fb").unwrap(), // User Description
                    value: b"Fragment Transmit".to_vec(),
                }
            ],
        });
        
        // 2. Fragment RX Characteristic (Read, Notify)
        characteristics.push(GattCharacteristic {
            uuid: Uuid::parse_str(FRAGMENT_RX_CHAR_UUID).unwrap(),
            properties: CharacteristicProperty::Read as u8 
                | CharacteristicProperty::Notify as u8,
            max_length: 512,
            value: Vec::new(),
            descriptors: vec![
                GattDescriptor {
                    uuid: Uuid::parse_str("00002902-0000-1000-8000-00805f9b34fb").unwrap(), // CCCD
                    value: vec![0, 0],
                },
                GattDescriptor {
                    uuid: Uuid::parse_str("00002901-0000-1000-8000-00805f9b34fb").unwrap(), // User Description
                    value: b"Fragment Receive".to_vec(),
                }
            ],
        });
        
        // 3. Device Info Characteristic (Read)
        characteristics.push(GattCharacteristic {
            uuid: Uuid::parse_str(DEVICE_INFO_CHAR_UUID).unwrap(),
            properties: CharacteristicProperty::Read as u8,
            max_length: 256,
            value: Vec::new(), // Will be set during initialization
            descriptors: vec![
                GattDescriptor {
                    uuid: Uuid::parse_str("00002901-0000-1000-8000-00805f9b34fb").unwrap(),
                    value: b"Device Information".to_vec(),
                }
            ],
        });
        
        // 4. Confirmation Characteristic (Write, Notify)
        characteristics.push(GattCharacteristic {
            uuid: Uuid::parse_str(CONFIRMATION_CHAR_UUID).unwrap(),
            properties: CharacteristicProperty::Write as u8 
                | CharacteristicProperty::Notify as u8,
            max_length: 256,
            value: Vec::new(),
            descriptors: vec![
                GattDescriptor {
                    uuid: Uuid::parse_str("00002902-0000-1000-8000-00805f9b34fb").unwrap(), // CCCD
                    value: vec![0, 0],
                },
                GattDescriptor {
                    uuid: Uuid::parse_str("00002901-0000-1000-8000-00805f9b34fb").unwrap(),
                    value: b"Transaction Confirmation".to_vec(),
                }
            ],
        });
        
        // 5. Status Characteristic (Read, Notify)
        characteristics.push(GattCharacteristic {
            uuid: Uuid::parse_str(STATUS_CHAR_UUID).unwrap(),
            properties: CharacteristicProperty::Read as u8 
                | CharacteristicProperty::Notify as u8,
            max_length: 128,
            value: Vec::new(),
            descriptors: vec![
                GattDescriptor {
                    uuid: Uuid::parse_str("00002902-0000-1000-8000-00805f9b34fb").unwrap(), // CCCD
                    value: vec![0, 0],
                },
                GattDescriptor {
                    uuid: Uuid::parse_str("00002901-0000-1000-8000-00805f9b34fb").unwrap(),
                    value: b"Device Status".to_vec(),
                }
            ],
        });
        
        Self {
            service_uuid,
            characteristics,
        }
    }
    
    /// Get characteristic by UUID
    pub fn get_characteristic(&self, uuid: &Uuid) -> Option<&GattCharacteristic> {
        self.characteristics.iter().find(|c| &c.uuid == uuid)
    }
    
    /// Get mutable characteristic by UUID
    pub fn get_characteristic_mut(&mut self, uuid: &Uuid) -> Option<&mut GattCharacteristic> {
        self.characteristics.iter_mut().find(|c| &c.uuid == uuid)
    }
}

impl DeviceInfo {
    /// Create device info with default capabilities
    pub fn new(device_id: String, max_mtu: u16) -> Self {
        Self {
            device_id,
            capabilities: vec![
                DeviceCapability::CanRelay,
                DeviceCapability::SupportsCompression,
            ],
            protocol_version: "1.0.0".to_string(),
            max_mtu,
        }
    }
    
    /// Add a capability
    pub fn add_capability(&mut self, capability: DeviceCapability) {
        if !self.capabilities.iter().any(|c| matches!(c, capability)) {
            self.capabilities.push(capability);
        }
    }
    
    /// Check if device has a capability
    pub fn has_capability(&self, capability: &DeviceCapability) -> bool {
        self.capabilities.iter().any(|c| matches!(c, capability))
    }
    
    /// Serialize to bytes for characteristic value
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
    
    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}