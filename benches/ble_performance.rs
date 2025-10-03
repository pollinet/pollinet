//! BLE performance benchmarks for PolliNet SDK

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pollinet::{transaction::Fragment, PolliNetSDK};
use std::time::Duration;

fn benchmark_ble_discovery(c: &mut Criterion) {
    c.bench_function("ble_peer_discovery", |b| {
        b.iter(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                // Mock BLE discovery - in real tests this would use actual BLE
                let mock_peers = vec![
                    pollinet::ble::PeerInfo {
                        device_id: "peer_1".to_string(),
                        capabilities: vec!["relay".to_string()],
                        rssi: -50,
                        last_seen: std::time::Instant::now(),
                    },
                    pollinet::ble::PeerInfo {
                        device_id: "peer_2".to_string(),
                        capabilities: vec!["relay".to_string()],
                        rssi: -60,
                        last_seen: std::time::Instant::now(),
                    },
                ];

                // Simulate discovery latency
                tokio::time::sleep(Duration::from_millis(10)).await;
                black_box(mock_peers)
            })
        })
    });
}

fn benchmark_fragment_relay(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragment_relay");

    for fragment_count in [1, 5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::new("relay_fragments", fragment_count),
            fragment_count,
            |b, &fragment_count| {
                b.iter(|| {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        let fragments: Vec<Fragment> = (0..fragment_count)
                            .map(|i| Fragment {
                                id: format!("tx_{}", i),
                                index: i,
                                total: fragment_count,
                                data: vec![0u8; 100], // 100 bytes per fragment
                                fragment_type: if i == 0 {
                                    pollinet::transaction::FragmentType::FragmentStart
                                } else if i == fragment_count - 1 {
                                    pollinet::transaction::FragmentType::FragmentEnd
                                } else {
                                    pollinet::transaction::FragmentType::FragmentContinue
                                },
                            })
                            .collect();

                        // Simulate relay processing time
                        tokio::time::sleep(Duration::from_micros(100 * fragment_count as u64)).await;
                        black_box(fragments)
                    })
                })
            },
        );
    }
    group.finish();
}

fn benchmark_ble_connection(c: &mut Criterion) {
    c.bench_function("ble_connection_establishment", |b| {
        b.iter(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                // Mock connection establishment
                let peer_id = "mock_peer_123";

                // Simulate connection time
                tokio::time::sleep(Duration::from_millis(50)).await;
                black_box(peer_id)
            })
        })
    });
}

criterion_group!(
    benches,
    benchmark_ble_discovery,
    benchmark_fragment_relay,
    benchmark_ble_connection
);
criterion_main!(benches);
