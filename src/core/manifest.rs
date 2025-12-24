//! Cartridge container manifest
//!
//! Provides npm-style package metadata for Cartridge containers.
//! The manifest distinguishes between:
//! - **Slug**: Kebab-case identifier (filename, registry key, canonical reference)
//! - **Title**: Human-readable display name

use crate::error::Result;
use crate::validation::ContainerSlug;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Cartridge container manifest
///
/// Similar to npm's package.json, provides metadata about a Cartridge container.
///
/// # Key Distinction: Slug vs Title
///
/// - **slug**: Normalized kebab-case identifier ("us-const")
///   - Used for: filename, registry key, canonical references
///   - Must be valid kebab-case (lowercase, hyphens only)
///
/// - **title**: Human-readable display name ("U.S. Constitution")
///   - Used for: UI display, documentation, user-facing names
///   - Can contain any characters, spaces, capitalization
///
/// # Examples
///
/// ```
/// use cartridge_core::manifest::Manifest;
/// use semver::Version;
///
/// let manifest = Manifest::new(
///     "us-const",
///     "U.S. Constitution",
///     Version::new(0, 1, 0)
/// ).unwrap();
///
/// assert_eq!(manifest.slug.as_str(), "us-const");
/// assert_eq!(manifest.title, "U.S. Constitution");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Container slug (kebab-case identifier)
    ///
    /// Used for filenames, registry keys, and canonical references.
    /// Must be valid kebab-case: lowercase, numbers, hyphens only.
    ///
    /// Example: "us-const"
    #[serde(
        serialize_with = "serialize_slug",
        deserialize_with = "deserialize_slug"
    )]
    pub slug: ContainerSlug,

    /// Container title (human-readable display name)
    ///
    /// Used for UI display and documentation.
    /// Can contain any characters, spaces, capitalization.
    ///
    /// Example: "U.S. Constitution"
    pub title: String,

    /// Semantic version
    ///
    /// Example: "0.1.0", "1.2.3", "2.0.0-beta.1"
    pub version: Version,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Author information
    ///
    /// Format: "Name <email@example.com>"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    /// License identifier (SPDX)
    ///
    /// Examples: "MIT", "Apache-2.0", "MIT OR Apache-2.0"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// Creation timestamp (ISO 8601)
    ///
    /// Example: "2025-11-21T12:00:00Z"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,

    /// Repository URL
    ///
    /// Example: "https://github.com/org/repo"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,

    /// Dependencies (slug -> version requirement)
    ///
    /// Example: { "other-container": "^1.0.0" }
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub dependencies: HashMap<String, String>,

    /// IAM capabilities
    ///
    /// Example: ["read:public/*", "write:data/*"]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,

    /// Custom metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Manifest {
    /// Manifest file path inside container
    pub const PATH: &'static str = "/.cartridge/manifest.json";

    /// Create a new manifest with required fields
    ///
    /// # Arguments
    ///
    /// * `slug` - Kebab-case identifier (validated)
    /// * `title` - Human-readable display name
    /// * `version` - Semantic version
    ///
    /// # Errors
    ///
    /// Returns error if slug is not valid kebab-case.
    ///
    /// # Examples
    ///
    /// ```
    /// use cartridge_core::manifest::Manifest;
    /// use semver::Version;
    ///
    /// let manifest = Manifest::new(
    ///     "my-container",
    ///     "My Container",
    ///     Version::new(1, 0, 0)
    /// ).unwrap();
    /// ```
    pub fn new(
        slug: impl Into<String>,
        title: impl Into<String>,
        version: Version,
    ) -> Result<Self> {
        let slug = ContainerSlug::new(slug)?;
        let now = chrono::Utc::now().to_rfc3339();

        Ok(Self {
            slug,
            title: title.into(),
            version,
            description: None,
            author: None,
            license: None,
            created: Some(now),
            repository: None,
            dependencies: HashMap::new(),
            capabilities: Vec::new(),
            metadata: HashMap::new(),
        })
    }

    /// Validate all fields
    ///
    /// Checks:
    /// - Slug is valid kebab-case
    /// - Dependencies have valid slugs
    /// - Dependencies have valid version requirements
    pub fn validate(&self) -> Result<()> {
        // Slug is validated in ContainerSlug::new()

        // Validate dependency slugs
        for (dep_slug, _dep_version) in &self.dependencies {
            ContainerSlug::new(dep_slug)?;
            // TODO: Validate version requirement syntax
        }

        Ok(())
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set author
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Set license
    pub fn with_license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    /// Set repository
    pub fn with_repository(mut self, repository: impl Into<String>) -> Self {
        self.repository = Some(repository.into());
        self
    }

    /// Add a dependency
    pub fn add_dependency(
        mut self,
        slug: impl Into<String>,
        version_req: impl Into<String>,
    ) -> Result<Self> {
        let slug = slug.into();
        ContainerSlug::new(&slug)?; // Validate dependency slug
        self.dependencies.insert(slug, version_req.into());
        Ok(self)
    }

    /// Add a capability
    pub fn add_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.push(capability.into());
        self
    }

    /// Add custom metadata
    pub fn add_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

// Custom serialization for ContainerSlug
fn serialize_slug<S>(slug: &ContainerSlug, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(slug.as_str())
}

// Custom deserialization for ContainerSlug
fn deserialize_slug<'de, D>(deserializer: D) -> std::result::Result<ContainerSlug, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    ContainerSlug::new(s).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_manifest() -> Result<()> {
        let manifest = Manifest::new("my-container", "My Container", Version::new(1, 0, 0))?;

        assert_eq!(manifest.slug.as_str(), "my-container");
        assert_eq!(manifest.title, "My Container");
        assert_eq!(manifest.version, Version::new(1, 0, 0));

        Ok(())
    }

    #[test]
    fn test_slug_vs_title() -> Result<()> {
        let manifest = Manifest::new("us-const", "U.S. Constitution", Version::new(0, 1, 0))?;

        // Slug is kebab-case (for filenames, registry)
        assert_eq!(manifest.slug.as_str(), "us-const");

        // Title is human-readable (for display)
        assert_eq!(manifest.title, "U.S. Constitution");

        Ok(())
    }

    #[test]
    fn test_builder_pattern() -> Result<()> {
        let manifest = Manifest::new("test-container", "Test Container", Version::new(1, 0, 0))?
            .with_description("A test container")
            .with_author("Test Author <test@example.com>")
            .with_license("MIT")
            .with_repository("https://github.com/test/repo")
            .add_capability("read:public/*")
            .add_dependency("other-container", "^1.0.0")?;

        assert_eq!(manifest.description, Some("A test container".to_string()));
        assert_eq!(
            manifest.author,
            Some("Test Author <test@example.com>".to_string())
        );
        assert!(manifest.dependencies.contains_key("other-container"));

        Ok(())
    }

    #[test]
    fn test_serialize_deserialize() -> Result<()> {
        let manifest = Manifest::new("my-container", "My Container", Version::new(1, 2, 3))?;

        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let deserialized: Manifest = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.slug.as_str(), "my-container");
        assert_eq!(deserialized.title, "My Container");
        assert_eq!(deserialized.version, Version::new(1, 2, 3));

        Ok(())
    }

    #[test]
    fn test_invalid_slug() {
        // Invalid slug (uppercase)
        let result = Manifest::new("My-Container", "My Container", Version::new(1, 0, 0));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_dependencies() -> Result<()> {
        let manifest = Manifest::new("test", "Test", Version::new(1, 0, 0))?
            .add_dependency("valid-dep", "^1.0.0")?;

        assert!(manifest.validate().is_ok());

        Ok(())
    }
}
