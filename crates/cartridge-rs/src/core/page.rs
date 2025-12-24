use crate::error::{CartridgeError, Result};
use crate::header::PAGE_SIZE;
use sha2::{Digest, Sha256};

/// Page types in the cartridge archive
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PageType {
    /// Header page (always page 0)
    Header = 0,
    /// B-tree catalog node
    CatalogBTree = 1,
    /// Content data block
    ContentData = 2,
    /// Free list page
    Freelist = 3,
    /// Audit log entry
    AuditLog = 4,
}

impl PageType {
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            0 => Ok(PageType::Header),
            1 => Ok(PageType::CatalogBTree),
            2 => Ok(PageType::ContentData),
            3 => Ok(PageType::Freelist),
            4 => Ok(PageType::AuditLog),
            _ => Err(CartridgeError::InvalidPageType(value)),
        }
    }
}

/// Page header (64 bytes)
///
/// Each page begins with this header containing:
/// - Page type identifier
/// - SHA-256 checksum (optional verification)
/// - Reserved space for future use
#[repr(C)]
#[derive(Debug, Clone)]
pub struct PageHeader {
    /// Type of this page
    pub page_type: PageType,

    /// SHA-256 checksum of page data (32 bytes)
    /// Optional verification - can be all zeros to skip
    pub checksum: [u8; 32],

    /// Reserved for future use (31 bytes)
    pub reserved: [u8; 31],
}

impl PageHeader {
    pub fn new(page_type: PageType) -> Self {
        PageHeader {
            page_type,
            checksum: [0; 32],
            reserved: [0; 31],
        }
    }

    /// Size of the header in bytes
    pub const fn size() -> usize {
        1 + 32 + 31 // page_type + checksum + reserved
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::size());
        bytes.push(self.page_type as u8);
        bytes.extend_from_slice(&self.checksum);
        bytes.extend_from_slice(&self.reserved);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < Self::size() {
            return Err(CartridgeError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Insufficient bytes for page header",
            )));
        }

        let page_type = PageType::from_u8(bytes[0])?;
        let mut checksum = [0u8; 32];
        checksum.copy_from_slice(&bytes[1..33]);
        let mut reserved = [0u8; 31];
        reserved.copy_from_slice(&bytes[33..64]);

        Ok(PageHeader {
            page_type,
            checksum,
            reserved,
        })
    }
}

/// A single page in the cartridge archive
///
/// Pages are the fundamental storage unit (4KB each).
/// Structure: [Header (64 bytes)][Data (4032 bytes)]
pub struct Page {
    pub header: PageHeader,
    pub data: Vec<u8>,
}

impl Page {
    /// Create a new page of given type
    pub fn new(page_type: PageType) -> Self {
        let data_size = PAGE_SIZE - PageHeader::size();
        Page {
            header: PageHeader::new(page_type),
            data: vec![0; data_size],
        }
    }

    /// Create a page with specific data
    pub fn with_data(page_type: PageType, data: Vec<u8>) -> Result<Self> {
        let max_data_size = PAGE_SIZE - PageHeader::size();
        if data.len() > max_data_size {
            return Err(CartridgeError::Allocation(format!(
                "Page data too large: {} bytes (max: {})",
                data.len(),
                max_data_size
            )));
        }

        let mut padded_data = data;
        padded_data.resize(max_data_size, 0);

        Ok(Page {
            header: PageHeader::new(page_type),
            data: padded_data,
        })
    }

    /// Compute and update the SHA-256 checksum of the page data
    pub fn compute_checksum(&mut self) {
        let mut hasher = Sha256::new();
        hasher.update(&self.data);
        self.header.checksum = hasher.finalize().into();
    }

    /// Verify the page checksum matches the data
    ///
    /// Returns true if checksum matches or if checksum is all zeros (verification skipped).
    pub fn verify_checksum(&self) -> bool {
        // Skip verification if checksum is all zeros
        if self.header.checksum == [0u8; 32] {
            return true;
        }

        let mut hasher = Sha256::new();
        hasher.update(&self.data);
        let computed: [u8; 32] = hasher.finalize().into();

        computed == self.header.checksum
    }

    /// Serialize page to bytes (full PAGE_SIZE)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(PAGE_SIZE);
        bytes.extend_from_slice(&self.header.to_bytes());
        bytes.extend_from_slice(&self.data);
        assert_eq!(bytes.len(), PAGE_SIZE);
        bytes
    }

    /// Deserialize page from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < PAGE_SIZE {
            return Err(CartridgeError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "Page bytes too short: {} (expected {})",
                    bytes.len(),
                    PAGE_SIZE
                ),
            )));
        }

        let header = PageHeader::from_bytes(&bytes[..PageHeader::size()])?;
        let data = bytes[PageHeader::size()..PAGE_SIZE].to_vec();

        Ok(Page { header, data })
    }

    /// Get the page type
    pub fn page_type(&self) -> PageType {
        self.header.page_type
    }

    /// Get the data size (excluding header)
    pub fn data_size(&self) -> usize {
        self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_type_conversion() {
        assert_eq!(PageType::from_u8(0).unwrap(), PageType::Header);
        assert_eq!(PageType::from_u8(1).unwrap(), PageType::CatalogBTree);
        assert_eq!(PageType::from_u8(2).unwrap(), PageType::ContentData);
        assert_eq!(PageType::from_u8(3).unwrap(), PageType::Freelist);
        assert_eq!(PageType::from_u8(4).unwrap(), PageType::AuditLog);
        assert!(PageType::from_u8(99).is_err());
    }

    #[test]
    fn test_page_header_size() {
        assert_eq!(PageHeader::size(), 64);
    }

    #[test]
    fn test_page_creation() {
        let page = Page::new(PageType::ContentData);
        assert_eq!(page.page_type(), PageType::ContentData);
        assert_eq!(page.data.len(), PAGE_SIZE - PageHeader::size());
    }

    #[test]
    fn test_page_checksum() {
        let mut page = Page::new(PageType::ContentData);

        // Initially no checksum
        assert_eq!(page.header.checksum, [0u8; 32]);
        assert!(page.verify_checksum()); // Skipped verification

        // Add some data
        page.data[0] = 42;
        page.data[100] = 99;

        // Compute checksum
        page.compute_checksum();
        assert_ne!(page.header.checksum, [0u8; 32]);
        assert!(page.verify_checksum());

        // Corrupt data
        page.data[0] = 43;
        assert!(!page.verify_checksum());
    }

    #[test]
    fn test_page_serialization() {
        let mut page = Page::new(PageType::CatalogBTree);
        page.data[0] = 1;
        page.data[1] = 2;
        page.data[2] = 3;
        page.compute_checksum();

        let bytes = page.to_bytes();
        assert_eq!(bytes.len(), PAGE_SIZE);

        let deserialized = Page::from_bytes(&bytes).unwrap();
        assert_eq!(deserialized.page_type(), PageType::CatalogBTree);
        assert_eq!(deserialized.data[0], 1);
        assert_eq!(deserialized.data[1], 2);
        assert_eq!(deserialized.data[2], 3);
        assert!(deserialized.verify_checksum());
    }

    #[test]
    fn test_page_with_data() {
        let data = vec![1, 2, 3, 4, 5];
        let page = Page::with_data(PageType::ContentData, data.clone()).unwrap();

        assert_eq!(page.page_type(), PageType::ContentData);
        assert_eq!(page.data[0], 1);
        assert_eq!(page.data[4], 5);
        // Rest should be padded with zeros
        assert_eq!(page.data[5], 0);
    }

    #[test]
    fn test_page_data_too_large() {
        let max_size = PAGE_SIZE - PageHeader::size();
        let data = vec![0u8; max_size + 1];

        let result = Page::with_data(PageType::ContentData, data);
        assert!(matches!(result, Err(CartridgeError::Allocation(_))));
    }
}
