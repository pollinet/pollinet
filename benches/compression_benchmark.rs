//! Compression performance benchmarks for PolliNet SDK

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pollinet::util::lz::compress_data;

fn benchmark_compression_ratios(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_ratios");

    // Test different data sizes
    for size in [100, 500, 1000, 5000, 10000].iter() {
        let data = generate_test_data(*size);

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            BenchmarkId::new("lz4_compression", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let compressed = compress_data(black_box(data)).unwrap();
                    black_box(compressed)
                })
            },
        );
    }
    group.finish();
}

fn benchmark_compression_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_types");

    // Test different data patterns
    let test_cases = vec![
        ("random_data", generate_random_data(1000)),
        ("repetitive_data", generate_repetitive_data(1000)),
        ("json_like_data", generate_json_like_data(1000)),
        ("binary_data", generate_binary_data(1000)),
    ];

    for (name, data) in test_cases {
        group.bench_with_input(BenchmarkId::new("compress", name), &data, |b, data| {
            b.iter(|| {
                let compressed = compress_data(black_box(data)).unwrap();
                black_box(compressed)
            })
        });
    }
    group.finish();
}

fn benchmark_decompression(c: &mut Criterion) {
    let original_data = generate_test_data(1000);
    let compressed_data = compress_data(&original_data).unwrap();

    c.bench_function("lz4_decompression", |b| {
        b.iter(|| {
            let decompressed =
                pollinet::util::lz::decompress_data(black_box(&compressed_data)).unwrap();
            black_box(decompressed)
        })
    });
}

// Helper functions to generate test data
fn generate_test_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

fn generate_random_data(size: usize) -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    (0..size)
        .map(|i| {
            let mut hasher = DefaultHasher::new();
            i.hash(&mut hasher);
            (hasher.finish() % 256) as u8
        })
        .collect()
}

fn generate_repetitive_data(size: usize) -> Vec<u8> {
    let pattern = b"PolliNet BLE mesh transaction relay ";
    (0..size).map(|i| pattern[i % pattern.len()]).collect()
}

fn generate_json_like_data(size: usize) -> Vec<u8> {
    let mut data = Vec::new();
    let mut i = 0;

    while data.len() < size {
        let json_entry = format!(
            r#"{{"transaction_id":"tx_{}","sender":"{}","recipient":"{}","amount":{},"timestamp":{}}}"#,
            i,
            format!("sender_{}", i),
            format!("recipient_{}", i),
            i * 1000,
            1234567890 + i
        );
        data.extend_from_slice(json_entry.as_bytes());
        i += 1;
    }

    data.truncate(size);
    data
}

fn generate_binary_data(size: usize) -> Vec<u8> {
    // Simulate Solana transaction-like binary data
    let mut data = Vec::with_capacity(size);

    // Add some structure similar to Solana transactions
    for i in 0..size {
        match i % 64 {
            0..=31 => data.push((i % 256) as u8), // Simulated pubkey
            32..=63 => data.push(0xFF),           // Simulated signature padding
            _ => data.push((i * 7 % 256) as u8),  // Simulated instruction data
        }
    }

    data
}

criterion_group!(
    benches,
    benchmark_compression_ratios,
    benchmark_compression_types,
    benchmark_decompression
);
criterion_main!(benches);
