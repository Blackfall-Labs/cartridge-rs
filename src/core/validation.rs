//! Validation for Cartridge container names and paths
//!
//! This module provides strict validation for container slugs (kebab-case identifiers)
//! and path normalization to ensure consistent naming across the ecosystem.

use crate::error::{CartridgeError, Result};
use regex::Regex;
use std::path::{Path, PathBuf};

/// Validates a container slug (kebab-case identifier)
///
/// A slug is a normalized, URL-safe identifier used for:
/// - Filenames (e.g., "my-container.cart")
/// - Registry keys
/// - Canonical references
///
/// # Rules
/// - Lowercase letters (a-z), numbers (0-9), hyphens (-) only
/// - Must start and end with letter or number (not hyphen)
/// - No consecutive hyphens
/// - Length: 1-214 characters (npm package limit)
///
/// # Examples
///
/// Valid slugs:
/// - "my-container"
/// - "test123"
/// - "a"
/// - "my-cool-container-2"
///
/// Invalid slugs:
/// - "My-Container" (uppercase)
/// - "test_123" (underscores)
/// - "-test" (leading hyphen)
/// - "test-" (trailing hyphen)
/// - "my--container" (consecutive hyphens)
/// - "test.container" (dots)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerSlug(String);

impl ContainerSlug {
    /// Pattern for valid kebab-case slugs
    const PATTERN: &'static str = r"^[a-z0-9]([a-z0-9-]*[a-z0-9])?$";

    /// Maximum length (npm package name limit)
    const MAX_LENGTH: usize = 214;

    /// Create a new validated slug
    ///
    /// # Errors
    ///
    /// Returns `InvalidContainerSlug` if the slug doesn't meet validation rules.
    ///
    /// # Examples
    ///
    /// ```
    /// use cartridge_core::validation::ContainerSlug;
    ///
    /// let slug = ContainerSlug::new("my-container").unwrap();
    /// assert_eq!(slug.as_str(), "my-container");
    ///
    /// assert!(ContainerSlug::new("My-Container").is_err()); // uppercase
    /// assert!(ContainerSlug::new("my--container").is_err()); // consecutive hyphens
    /// ```
    pub fn new(slug: impl Into<String>) -> Result<Self> {
        let slug = slug.into();
        Self::validate_slug(&slug)?;
        Ok(ContainerSlug(slug))
    }

    /// Validate a slug string
    fn validate_slug(slug: &str) -> Result<()> {
        // Length check
        if slug.is_empty() {
            return Err(CartridgeError::InvalidContainerSlug(
                "slug cannot be empty".to_string(),
            ));
        }

        if slug.len() > Self::MAX_LENGTH {
            return Err(CartridgeError::InvalidContainerSlug(format!(
                "slug too long (max {} characters)",
                Self::MAX_LENGTH
            )));
        }

        // Pattern check (kebab-case)
        let re = Regex::new(Self::PATTERN).unwrap();
        if !re.is_match(slug) {
            return Err(CartridgeError::InvalidContainerSlug(format!(
                "slug '{}' must be kebab-case: lowercase letters, numbers, and hyphens only",
                slug
            )));
        }

        // No consecutive hyphens
        if slug.contains("--") {
            return Err(CartridgeError::InvalidContainerSlug(
                "slug cannot contain consecutive hyphens".to_string(),
            ));
        }

        Ok(())
    }

    /// Get the slug as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to String
    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for ContainerSlug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContainerSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Normalize a container path
///
/// Takes a slug/path input (WITHOUT .cart extension) and:
/// 1. Extracts the slug (if it's a path)
/// 2. Validates it as kebab-case
/// 3. ALWAYS appends `.cart` extension
/// 4. Returns normalized PathBuf
///
/// **IMPORTANT**: Users should NEVER specify `.cart` extension.
/// The core handles this internally to prevent extension mistakes.
///
/// # Examples
///
/// ```
/// use cartridge_core::validation::normalize_container_path;
/// use std::path::Path;
///
/// // User provides slug only
/// let path = normalize_container_path(Path::new("my-container")).unwrap();
/// assert_eq!(path, Path::new("my-container.cart"));
///
/// // Works with directory paths
/// let path = normalize_container_path(Path::new("/data/my-container")).unwrap();
/// assert_eq!(path, Path::new("/data/my-container.cart"));
///
/// // If user accidentally includes .cart, we strip and re-add it
/// let path = normalize_container_path(Path::new("my-container.cart")).unwrap();
/// assert_eq!(path, Path::new("my-container.cart"));
///
/// // Validates slug
/// assert!(normalize_container_path(Path::new("My-Container")).is_err());
/// ```
pub fn normalize_container_path(path: &Path) -> Result<PathBuf> {
    // Extract parent directory (if any)
    let parent = path.parent();

    // Extract filename (slug) - strip any extension if present
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or(CartridgeError::InvalidPath)?;

    // Validate as slug (no matter what extension was provided)
    ContainerSlug::new(file_stem)?;

    // Build normalized path: parent + slug + .cart
    let mut normalized = if let Some(parent_dir) = parent {
        parent_dir.join(file_stem)
    } else {
        PathBuf::from(file_stem)
    };

    // ALWAYS set extension to .cart (overwrite any user-provided extension)
    normalized.set_extension("cart");

    Ok(normalized)
}

/// Extract slug from a path
///
/// Extracts the slug (filename without extension) and validates it.
/// Works regardless of whether `.cart` extension is present.
///
/// # Examples
///
/// ```
/// use cartridge_core::validation::extract_slug;
/// use std::path::Path;
///
/// // With .cart extension
/// let slug = extract_slug(Path::new("my-container.cart")).unwrap();
/// assert_eq!(slug.as_str(), "my-container");
///
/// // Without .cart extension
/// let slug = extract_slug(Path::new("my-container")).unwrap();
/// assert_eq!(slug.as_str(), "my-container");
///
/// // With full path
/// let slug = extract_slug(Path::new("/path/to/my-container.cart")).unwrap();
/// assert_eq!(slug.as_str(), "my-container");
/// ```
pub fn extract_slug(path: &Path) -> Result<ContainerSlug> {
    // Always use file_stem to strip any extension
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or(CartridgeError::InvalidPath)?;

    ContainerSlug::new(file_stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_slugs() {
        assert!(ContainerSlug::new("test").is_ok());
        assert!(ContainerSlug::new("my-container").is_ok());
        assert!(ContainerSlug::new("test123").is_ok());
        assert!(ContainerSlug::new("a").is_ok());
        assert!(ContainerSlug::new("my-cool-container-2").is_ok());
    }

    #[test]
    fn test_invalid_slugs() {
        assert!(ContainerSlug::new("").is_err()); // empty
        assert!(ContainerSlug::new("My-Container").is_err()); // uppercase
        assert!(ContainerSlug::new("test_123").is_err()); // underscore
        assert!(ContainerSlug::new("-test").is_err()); // leading hyphen
        assert!(ContainerSlug::new("test-").is_err()); // trailing hyphen
        assert!(ContainerSlug::new("my--container").is_err()); // consecutive hyphens
        assert!(ContainerSlug::new("test.container").is_err()); // dot
        assert!(ContainerSlug::new("test container").is_err()); // space
    }

    #[test]
    fn test_normalize_path() {
        // User provides slug only - .cart is added
        let path = normalize_container_path(Path::new("my-container")).unwrap();
        assert_eq!(path, Path::new("my-container.cart"));

        // Even if user provides .cart, it's handled correctly
        let path = normalize_container_path(Path::new("my-container.cart")).unwrap();
        assert_eq!(path, Path::new("my-container.cart"));

        // Works with directory paths
        let path = normalize_container_path(Path::new("/data/my-container")).unwrap();
        assert_eq!(path, Path::new("/data/my-container.cart"));

        // Validates slug (rejects invalid)
        assert!(normalize_container_path(Path::new("My-Container")).is_err());
        assert!(normalize_container_path(Path::new("test_123")).is_err());

        // Rejects wrong extensions (validates slug only)
        let path = normalize_container_path(Path::new("my-container.wrong")).unwrap();
        assert_eq!(path, Path::new("my-container.cart")); // Corrects to .cart
    }

    #[test]
    fn test_extract_slug() {
        let slug = extract_slug(Path::new("my-container.cart")).unwrap();
        assert_eq!(slug.as_str(), "my-container");

        let slug = extract_slug(Path::new("/path/to/my-container.cart")).unwrap();
        assert_eq!(slug.as_str(), "my-container");

        let slug = extract_slug(Path::new("my-container")).unwrap();
        assert_eq!(slug.as_str(), "my-container");
    }
}
