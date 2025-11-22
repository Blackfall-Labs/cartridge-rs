//! Disk I/O operations for cartridge archives

use crate::error::{CartridgeError, Result};
use crate::header::{Header, PAGE_SIZE};
use crate::page::Page;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Disk-backed cartridge storage
pub struct CartridgeFile {
    file: File,
    path: std::path::PathBuf,
}

impl CartridgeFile {
    /// Create a new cartridge file
    pub fn create<P: AsRef<Path>>(path: P, header: &Header) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;

        // Write header to page 0
        file.write_all(&header.to_bytes())?;
        file.flush()?;

        Ok(CartridgeFile {
            file,
            path: path.as_ref().to_path_buf(),
        })
    }

    /// Open an existing cartridge file
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(&path)?;

        Ok(CartridgeFile {
            file,
            path: path.as_ref().to_path_buf(),
        })
    }

    /// Read the header (page 0)
    pub fn read_header(&mut self) -> Result<Header> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut buffer = vec![0u8; PAGE_SIZE];
        self.file.read_exact(&mut buffer)?;
        Header::from_bytes(&buffer)
    }

    /// Write the header (page 0)
    pub fn write_header(&mut self, header: &Header) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header.to_bytes())?;
        self.file.flush()?;
        Ok(())
    }

    /// Read a page
    pub fn read_page(&mut self, page_id: u64) -> Result<Page> {
        let offset = page_id * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;

        let mut buffer = vec![0u8; PAGE_SIZE];
        self.file.read_exact(&mut buffer)?;

        Page::from_bytes(&buffer)
    }

    /// Write a page
    pub fn write_page(&mut self, page_id: u64, page: &Page) -> Result<()> {
        let offset = page_id * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;

        self.file.write_all(&page.to_bytes())?;
        self.file.flush()?;

        Ok(())
    }

    /// Read raw page data (for content blocks)
    pub fn read_page_data(&mut self, page_id: u64) -> Result<Vec<u8>> {
        let offset = page_id * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;

        let mut buffer = vec![0u8; PAGE_SIZE];
        self.file.read_exact(&mut buffer)?;

        Ok(buffer)
    }

    /// Write raw page data (for content blocks)
    pub fn write_page_data(&mut self, page_id: u64, data: &[u8]) -> Result<()> {
        if data.len() != PAGE_SIZE {
            return Err(CartridgeError::Allocation(format!(
                "Page data must be exactly {} bytes, got {}",
                PAGE_SIZE,
                data.len()
            )));
        }

        let offset = page_id * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;

        self.file.write_all(data)?;
        self.file.flush()?;

        Ok(())
    }

    /// Get file path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sync all writes to disk
    pub fn sync(&mut self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_create_and_read_header() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path();

        let mut header = Header::new();
        header.total_blocks = 1000;
        header.free_blocks = 500;

        let mut cart_file = CartridgeFile::create(path, &header).unwrap();
        let read_header = cart_file.read_header().unwrap();

        assert_eq!(read_header.total_blocks, 1000);
        assert_eq!(read_header.free_blocks, 500);
    }

    #[test]
    fn test_write_and_read_page_data() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path();

        let header = Header::new();
        let mut cart_file = CartridgeFile::create(path, &header).unwrap();

        // Write data to page 1
        let mut data = vec![0u8; PAGE_SIZE];
        data[0..5].copy_from_slice(b"Hello");

        cart_file.write_page_data(1, &data).unwrap();

        // Read it back
        let read_data = cart_file.read_page_data(1).unwrap();
        assert_eq!(&read_data[0..5], b"Hello");
    }

    #[test]
    fn test_open_existing() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();

        {
            let mut header = Header::new();
            header.total_blocks = 999;
            CartridgeFile::create(&path, &header).unwrap();
        }

        // Reopen
        let mut cart_file = CartridgeFile::open(&path).unwrap();
        let header = cart_file.read_header().unwrap();
        assert_eq!(header.total_blocks, 999);
    }
}
