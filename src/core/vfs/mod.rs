//! SQLite VFS (Virtual File System) implementation for Cartridge
//!
//! This module provides a custom SQLite VFS that allows SQLite to read and write
//! database files directly within a Cartridge archive. This enables:
//!
//! - Direct SQL queries on archived data
//! - ACID transactions within the archive
//! - WAL (Write-Ahead Logging) support
//! - No extraction required - SQLite I/O goes straight to Cartridge pages

mod file;
mod vfs;

#[cfg(test)]
mod tests;

pub use file::CartridgeFile;
pub use vfs::{register_vfs, unregister_vfs, CartridgeVFS, VFS_NAME};

use crate::error::CartridgeError;

pub type Result<T> = std::result::Result<T, CartridgeError>;
