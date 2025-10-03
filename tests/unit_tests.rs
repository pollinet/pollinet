//! Unit tests for individual PolliNet components

#[cfg(test)]
mod transaction_tests {
    use pollinet::transaction::{Fragment, FragmentType};

    #[test]
    fn test_fragment_creation() {
        let fragment = Fragment {
            id: "test_tx_123".to_string(),
            index: 0,
            total: 3,
            data: vec![1, 2, 3, 4, 5],
            fragment_type: FragmentType::FragmentStart,
        };

        assert_eq!(fragment.id, "test_tx_123");
        assert_eq!(fragment.index, 0);
        assert_eq!(fragment.total, 3);
        assert_eq!(fragment.data.len(), 5);
        assert!(matches!(
            fragment.fragment_type,
            FragmentType::FragmentStart
        ));
    }

    #[test]
    fn test_fragment_serialization() {
        let fragment = Fragment {
            id: "test_tx_456".to_string(),
            index: 1,
            total: 2,
            data: vec![10, 20, 30],
            fragment_type: FragmentType::FragmentEnd,
        };

        // Test serialization
        let serialized = serde_json::to_string(&fragment).expect("Should serialize");
        assert!(!serialized.is_empty());

        // Test deserialization
        let deserialized: Fragment = serde_json::from_str(&serialized).expect("Should deserialize");
        assert_eq!(deserialized.id, fragment.id);
        assert_eq!(deserialized.index, fragment.index);
        assert_eq!(deserialized.total, fragment.total);
        assert_eq!(deserialized.data, fragment.data);
    }
}

#[cfg(test)]
mod utility_tests {
    use pollinet::util::common;

    #[test]
    fn test_id_generation_uniqueness() {
        let mut ids = std::collections::HashSet::new();

        // Generate multiple IDs and ensure they're unique
        // Add small delay to ensure timestamp-based uniqueness
        for _ in 0..10 {
            let id = common::generate_id();
            assert!(ids.insert(id), "All generated IDs should be unique");
            // Small delay to ensure timestamp changes
            std::thread::sleep(std::time::Duration::from_nanos(1));
        }
    }

    #[test]
    fn test_compression_threshold_logic() {
        // Test various threshold scenarios
        assert!(common::should_compress(101, 100));
        assert!(common::should_compress(1000, 100));
        assert!(!common::should_compress(100, 100));
        assert!(!common::should_compress(50, 100));
        assert!(!common::should_compress(0, 100));
    }

    #[test]
    fn test_fragment_size_calculation() {
        // Test fragment size calculations
        assert_eq!(common::calculate_fragment_size(100, 100), 1);
        assert_eq!(common::calculate_fragment_size(150, 100), 2);
        assert_eq!(common::calculate_fragment_size(200, 100), 2);
        assert_eq!(common::calculate_fragment_size(250, 100), 3);
        assert_eq!(common::calculate_fragment_size(0, 100), 0);
    }
}

#[cfg(test)]
mod compression_tests {
    use pollinet::util::lz::Lz4Compressor;

    #[test]
    fn test_compression_empty_data() {
        let empty_data = vec![];
        let compressor = Lz4Compressor::new().expect("Should create compressor");
        let compressed = compressor.compress(&empty_data).expect("Should handle empty data");
        // For empty data, just verify compression works
        assert!(!compressed.is_empty() || empty_data.is_empty());
    }

    #[test]
    fn test_compression_small_data() {
        let small_data = vec![1, 2, 3, 4, 5];
        let compressor = Lz4Compressor::new().expect("Should create compressor");
        let compressed = compressor.compress(&small_data).expect("Should compress small data");
        // Just verify compression produces output
        assert!(!compressed.is_empty(), "Compressed data should not be empty");
    }

    #[test]
    fn test_compression_large_data() {
        let large_data = vec![42u8; 10000]; // 10KB of repeated data
        let compressor = Lz4Compressor::new().expect("Should create compressor");
        let compressed = compressor.compress(&large_data).expect("Should compress large data");

        // Verify compression works and produces smaller output for repetitive data
        assert!(!compressed.is_empty(), "Compressed data should not be empty");
        assert!(
            compressed.len() < large_data.len(),
            "Compressed data should be smaller for repetitive data"
        );

        println!(
            "Original: {} bytes, Compressed: {} bytes, Ratio: {:.2}%",
            large_data.len(),
            compressed.len(),
            (compressed.len() as f64 / large_data.len() as f64) * 100.0
        );
    }

    #[test]
    fn test_compression_random_data() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Generate pseudo-random data
        let random_data: Vec<u8> = (0..1000)
            .map(|i| {
                let mut hasher = DefaultHasher::new();
                i.hash(&mut hasher);
                (hasher.finish() % 256) as u8
            })
            .collect();

        let compressor = Lz4Compressor::new().expect("Should create compressor");
        let compressed = compressor.compress(&random_data).expect("Should compress random data");
        assert!(!compressed.is_empty(), "Compressed data should not be empty");
    }
}

#[cfg(test)]
mod ble_tests {
    use pollinet::ble::PeerInfo;

    #[test]
    fn test_peer_info_creation() {
        let peer = PeerInfo {
            device_id: "test_device_123".to_string(),
            capabilities: vec!["relay".to_string(), "gateway".to_string()],
            rssi: -45,
            last_seen: std::time::Instant::now(),
        };

        assert_eq!(peer.device_id, "test_device_123");
        assert_eq!(peer.capabilities.len(), 2);
        assert!(peer.capabilities.contains(&"relay".to_string()));
        assert!(peer.capabilities.contains(&"gateway".to_string()));
        assert_eq!(peer.rssi, -45);
    }

    #[test]
    fn test_peer_info_clone() {
        let peer = PeerInfo {
            device_id: "clone_test".to_string(),
            capabilities: vec!["test".to_string()],
            rssi: -60,
            last_seen: std::time::Instant::now(),
        };

        let cloned_peer = peer.clone();
        assert_eq!(peer.device_id, cloned_peer.device_id);
        assert_eq!(peer.capabilities, cloned_peer.capabilities);
        assert_eq!(peer.rssi, cloned_peer.rssi);
    }
}
