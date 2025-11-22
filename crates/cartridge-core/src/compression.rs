//! Transparent compression for cartridge content pages
//!
//! Provides LZ4 and Zstd compression for content pages to reduce storage usage.
//! Compression is transparent - reads automatically decompress, writes automatically compress.
//!
//! **Design**:
//! - Compression threshold: Only compress data >= 512 bytes (avoid overhead)
//! - Compression detection: PageType::CompressedData with method byte
//! - Format: [method: u8][compressed_size: u32][compressed_data]
//! - Fallback: Store uncompressed if compression ratio < 0.9

use crate::error::{CartridgeError, Result};
use crate::header::PAGE_SIZE;
use crate::page::PageHeader;

/// Compression method for content pages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressionMethod {
    /// No compression
    None = 0,
    /// LZ4 compression (fast, moderate ratio)
    Lz4 = 1,
    /// Zstd compression (slower, better ratio)
    Zstd = 2,
}

impl CompressionMethod {
    /// Convert from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(CompressionMethod::None),
            1 => Some(CompressionMethod::Lz4),
            2 => Some(CompressionMethod::Zstd),
            _ => None,
        }
    }
}

/// Compression configuration
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Compression method to use
    pub method: CompressionMethod,

    /// Minimum size to compress (bytes)
    /// Data smaller than this will not be compressed
    pub threshold: usize,

    /// Minimum compression ratio (compressed_size / original_size)
    /// If ratio is worse than this, store uncompressed
    pub min_ratio: f32,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        CompressionConfig {
            method: CompressionMethod::Lz4,
            threshold: 512,
            min_ratio: 0.9,
        }
    }
}

impl CompressionConfig {
    /// Create config with no compression
    pub fn none() -> Self {
        CompressionConfig {
            method: CompressionMethod::None,
            threshold: usize::MAX,
            min_ratio: 0.0,
        }
    }

    /// Create config with LZ4 compression
    pub fn lz4() -> Self {
        CompressionConfig {
            method: CompressionMethod::Lz4,
            ..Default::default()
        }
    }

    /// Create config with Zstd compression
    pub fn zstd() -> Self {
        CompressionConfig {
            method: CompressionMethod::Zstd,
            threshold: 1024, // Zstd overhead is higher
            min_ratio: 0.85, // Better compression ratio expected
        }
    }
}

/// Compress data using the specified method
pub fn compress(data: &[u8], method: CompressionMethod) -> Result<Vec<u8>> {
    match method {
        CompressionMethod::None => Ok(data.to_vec()),
        CompressionMethod::Lz4 => {
            let compressed = lz4_flex::compress_prepend_size(data);
            Ok(compressed)
        }
        CompressionMethod::Zstd => {
            let compressed = zstd::bulk::compress(data, 3).map_err(|e| {
                CartridgeError::Allocation(format!("Zstd compression failed: {}", e))
            })?;
            Ok(compressed)
        }
    }
}

/// Decompress data using the specified method
pub fn decompress(data: &[u8], method: CompressionMethod) -> Result<Vec<u8>> {
    match method {
        CompressionMethod::None => Ok(data.to_vec()),
        CompressionMethod::Lz4 => {
            let decompressed = lz4_flex::decompress_size_prepended(data).map_err(|e| {
                CartridgeError::Allocation(format!("LZ4 decompression failed: {}", e))
            })?;
            Ok(decompressed)
        }
        CompressionMethod::Zstd => {
            // For Zstd, we need to provide a max size. Use a generous limit for general use.
            // For page data, this will be (PAGE_SIZE - header) but for benchmarks and other uses
            // we may need larger buffers. Use 10MB as a reasonable upper bound.
            let max_size = 10 * 1024 * 1024; // 10MB max decompressed size
            let decompressed = zstd::bulk::decompress(data, max_size).map_err(|e| {
                CartridgeError::Allocation(format!("Zstd decompression failed: {}", e))
            })?;
            Ok(decompressed)
        }
    }
}

/// Compress data if beneficial, returns (data, method_used)
pub fn compress_if_beneficial(
    data: &[u8],
    config: &CompressionConfig,
) -> Result<(Vec<u8>, CompressionMethod)> {
    // Skip compression if below threshold
    if data.len() < config.threshold {
        return Ok((data.to_vec(), CompressionMethod::None));
    }

    // Skip if method is None
    if matches!(config.method, CompressionMethod::None) {
        return Ok((data.to_vec(), CompressionMethod::None));
    }

    // Try compression
    let compressed = compress(data, config.method)?;

    // Check compression ratio
    let ratio = compressed.len() as f32 / data.len() as f32;
    if ratio < config.min_ratio {
        // Compression was beneficial
        Ok((compressed, config.method))
    } else {
        // Compression not worth it, store uncompressed
        Ok((data.to_vec(), CompressionMethod::None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_method_conversion() {
        assert_eq!(CompressionMethod::from_u8(0), Some(CompressionMethod::None));
        assert_eq!(CompressionMethod::from_u8(1), Some(CompressionMethod::Lz4));
        assert_eq!(CompressionMethod::from_u8(2), Some(CompressionMethod::Zstd));
        assert_eq!(CompressionMethod::from_u8(99), None);
    }

    #[test]
    fn test_lz4_compression() {
        let data = b"Hello, World! ".repeat(100);
        let compressed = compress(&data, CompressionMethod::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionMethod::Lz4).unwrap();

        assert_eq!(data.as_slice(), decompressed.as_slice());
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_zstd_compression() {
        let data = b"Zstandard compression test data! ".repeat(100);
        let compressed = compress(&data, CompressionMethod::Zstd).unwrap();
        let decompressed = decompress(&compressed, CompressionMethod::Zstd).unwrap();

        assert_eq!(data.as_slice(), decompressed.as_slice());
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_compress_if_beneficial() {
        let config = CompressionConfig::lz4();

        // Small data - should not compress
        let small_data = b"Hello";
        let (result, method) = compress_if_beneficial(small_data, &config).unwrap();
        assert_eq!(method, CompressionMethod::None);
        assert_eq!(result, small_data);

        // Large repetitive data - should compress
        let large_data = b"X".repeat(2000);
        let (result, method) = compress_if_beneficial(&large_data, &config).unwrap();
        assert_eq!(method, CompressionMethod::Lz4);
        assert!(result.len() < large_data.len());
    }

    #[test]
    fn test_compression_config_defaults() {
        let config = CompressionConfig::default();
        assert_eq!(config.method, CompressionMethod::Lz4);
        assert_eq!(config.threshold, 512);
        assert_eq!(config.min_ratio, 0.9);

        let config = CompressionConfig::none();
        assert_eq!(config.method, CompressionMethod::None);

        let config = CompressionConfig::zstd();
        assert_eq!(config.method, CompressionMethod::Zstd);
    }

    #[test]
    fn test_no_compression() {
        let data = b"Test data";
        let compressed = compress(data, CompressionMethod::None).unwrap();
        let decompressed = decompress(&compressed, CompressionMethod::None).unwrap();

        assert_eq!(data, compressed.as_slice());
        assert_eq!(data, decompressed.as_slice());
    }

    #[test]
    fn test_compression_ratio_fallback() {
        let config = CompressionConfig {
            method: CompressionMethod::Lz4,
            threshold: 10,
            min_ratio: 0.9,
        };

        // Random data (incompressible)
        let random_data: Vec<u8> = (0..1000).map(|i| (i * 73) as u8).collect();
        let (result, method) = compress_if_beneficial(&random_data, &config).unwrap();

        // Should fallback to None if compression isn't beneficial
        if result.len() as f32 / random_data.len() as f32 >= 0.9 {
            assert_eq!(method, CompressionMethod::None);
        }
    }
}
