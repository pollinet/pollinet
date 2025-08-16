//! LZ4 compression utilities for PolliNet SDK
//!
//! Provides fast lossless compression for transaction payloads

use std::{i32, time::Instant};
use thiserror::Error;

/// LZ4 compressor for transaction payloads
pub struct Lz4Compressor;

impl Lz4Compressor {
    /// Create a new LZ4 compressor
    pub fn new() -> Result<Self, Lz4Error> {
        Ok(Self)
    }

    /// Compress data using LZ4
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>, Lz4Error> {
        let start_time = Instant::now();

        if data.is_empty() {
            return Ok(Vec::new());
        }

        // Use real LZ4 compression
        let compressed = lz4::block::compress(
            data,
            Some(lz4::block::CompressionMode::HIGHCOMPRESSION(i32::MAX)),
            false,
        )
        .map_err(|e| Lz4Error::CompressionFailed(e.to_string()))?;

        let compression_time = start_time.elapsed().as_micros();

        // Log compression statistics
        let ratio = self.get_compression_ratio(data.len(), compressed.len());
        tracing::debug!(
            "LZ4 compression: {} -> {} bytes ({}% reduction) in {}μs",
            data.len(),
            compressed.len(),
            ratio,
            compression_time
        );

        Ok(compressed)
    }

    /// Decompress LZ4 data
    pub fn decompress(&self, compressed_data: &[u8]) -> Result<Vec<u8>, Lz4Error> {
        if compressed_data.is_empty() {
            return Ok(Vec::new());
        }

        // Use real LZ4 decompression
        // Note: LZ4 block compression doesn't store original size, so we need to estimate
        // In production, you might want to store the original size in a header
        let decompressed = lz4::block::decompress(compressed_data, None)
            .map_err(|e| Lz4Error::DecompressionFailed(e.to_string()))?;

        Ok(decompressed)
    }

    /// Get compression ratio for given data
    pub fn get_compression_ratio(&self, original_size: usize, compressed_size: usize) -> f64 {
        if original_size == 0 {
            return 0.0;
        }

        let ratio = compressed_size as f64 / original_size as f64;
        (1.0 - ratio) * 100.0 // Return as percentage reduction
    }

    /// Check if data is compressed
    pub fn is_compressed(&self, data: &[u8]) -> bool {
        // For LZ4 block compression, we can't easily detect if data is compressed
        // In production, you might want to add a header or use a different approach
        // For now, we'll assume any non-empty data could be compressed
        !data.is_empty()
    }

    /// Compress with size estimation for decompression
    pub fn compress_with_size(&self, data: &[u8]) -> Result<Vec<u8>, Lz4Error> {
        let start_time = Instant::now();

        if data.is_empty() {
            return Ok(Vec::new());
        }

        // Create a header with original size for proper decompression
        let mut compressed = Vec::new();
        compressed.extend_from_slice(b"LZ4");
        compressed.extend_from_slice(&(data.len() as u32).to_le_bytes());

        // Compress the actual data
        let compressed_data =
            lz4::block::compress(data, Some(lz4::block::CompressionMode::DEFAULT), false)
                .map_err(|e| Lz4Error::CompressionFailed(e.to_string()))?;

        compressed.extend_from_slice(&compressed_data);

        let compression_time = start_time.elapsed().as_micros();

        // Log compression statistics
        let ratio = self.get_compression_ratio(data.len(), compressed.len());
        tracing::info!(
            "LZ4 compression with header: {} -> {} bytes ({}% reduction) in {}μs",
            data.len(),
            compressed.len(),
            ratio,
            compression_time
        );

        Ok(compressed)
    }

    /// Decompress data with size header
    pub fn decompress_with_size(&self, compressed_data: &[u8]) -> Result<Vec<u8>, Lz4Error> {
        if compressed_data.len() < 8 {
            return Err(Lz4Error::InvalidData(
                "Data too short for LZ4 header".to_string(),
            ));
        }

        // Check LZ4 header
        if &compressed_data[..4] != b"LZ4" {
            return Err(Lz4Error::InvalidData("Invalid LZ4 header".to_string()));
        }

        // Extract original size
        let original_size = u32::from_le_bytes([
            compressed_data[4],
            compressed_data[5],
            compressed_data[6],
            compressed_data[7],
        ]) as usize;

        // Extract compressed data
        let data = &compressed_data[8..];

        // Decompress
        let decompressed = lz4::block::decompress(data, Some(original_size as i32))
            .map_err(|e| Lz4Error::DecompressionFailed(e.to_string()))?;

        if decompressed.len() != original_size {
            return Err(Lz4Error::InvalidData(
                "Decompressed size mismatch".to_string(),
            ));
        }

        Ok(decompressed)
    }
}

/// LZ4-specific error types
#[derive(Error, Debug)]
pub enum Lz4Error {
    #[error("LZ4 initialization failed: {0}")]
    InitializationFailed(String),

    #[error("Compression failed: {0}")]
    CompressionFailed(String),

    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Memory allocation failed: {0}")]
    MemoryError(String),
}

/// Compression statistics
#[derive(Debug, Clone)]
pub struct CompressionStats {
    /// Original data size in bytes
    pub original_size: usize,
    /// Compressed data size in bytes
    pub compressed_size: usize,
    /// Compression ratio as percentage
    pub compression_ratio: f64,
    /// Compression time in microseconds
    pub compression_time: u64,
}

impl CompressionStats {
    /// Create new compression stats
    pub fn new(original_size: usize, compressed_size: usize, compression_time: u64) -> Self {
        let compression_ratio = if original_size > 0 {
            (1.0 - compressed_size as f64 / original_size as f64) * 100.0
        } else {
            0.0
        };

        Self {
            original_size,
            compressed_size,
            compression_ratio,
            compression_time,
        }
    }
}
