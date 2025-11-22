//! S3-compatible HTTP API for Cartridge storage
//!
//! This crate provides an S3-compatible HTTP interface to Cartridge,
//! enabling any S3 client (aws-cli, s3cmd, rclone, AWS SDKs) to work
//! with Cartridge storage.
//!
//! ## Architecture
//!
//! - **S3 Buckets** → Cartridge directories (e.g., `/mybucket/`)
//! - **S3 Objects** → Cartridge files (e.g., `/mybucket/file.txt`)
//! - **ETags** → SHA-256 content hash (already in Cartridge)
//! - **Metadata** → FileMetadata.user_metadata HashMap
//!
//! ## Example
//!
//! ```no_run
//! use cartridge_core::Cartridge;
//! use cartridge_s3::CartridgeS3Backend;
//! use parking_lot::RwLock;
//! use std::sync::Arc;
//!
//! # tokio_test::block_on(async {
//! // Create Cartridge instance
//! let cart = Cartridge::new(10000);
//! let cart_arc = Arc::new(RwLock::new(cart));
//!
//! // Create S3 backend
//! let backend = CartridgeS3Backend::new(cart_arc);
//!
//! // Use with s3s to create HTTP server
//! // (see examples/server.rs for full implementation)
//! # });
//! ```

mod acl;
mod backend;
mod error;
mod multipart;
mod s3_impl;
mod sse;
mod utils;
mod versioning;

pub use acl::{check_permission, S3Acl, S3Grant, S3Permission};
pub use backend::CartridgeS3Backend;
pub use error::{S3Error, S3Result};
pub use multipart::MultipartManager;
pub use sse::SseHeaders;
pub use versioning::{should_create_version, VersionId, VersioningManager};
