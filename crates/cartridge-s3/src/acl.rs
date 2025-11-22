//! S3 ACL (Access Control List) support
//!
//! Provides three modes:
//! - Ignore: Accept ACL APIs but don't store or enforce
//! - Record: Store ACLs in metadata but don't enforce
//! - Enforce: Store and enforce ACLs via IAM policy checks

use serde::{Deserialize, Serialize};

/// S3 ACL structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Acl {
    /// Object owner
    pub owner: Option<String>,
    /// Access grants
    pub grants: Vec<S3Grant>,
}

impl S3Acl {
    /// Create an empty ACL
    pub fn empty() -> Self {
        S3Acl {
            owner: None,
            grants: Vec::new(),
        }
    }

    /// Serialize to JSON for metadata storage
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON metadata
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// S3 grant structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Grant {
    /// Grantee identifier (user ID, email, or URI)
    pub grantee: String,
    /// Permission level
    pub permission: S3Permission,
}

/// S3 permission types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum S3Permission {
    /// Read object data
    Read,
    /// Write object data
    Write,
    /// Read ACL
    ReadAcp,
    /// Write ACL
    WriteAcp,
    /// Full control
    FullControl,
}

/// Check if a user has a specific permission
///
/// TODO: Full implementation would check grants and handle group permissions
pub fn check_permission(acl: &S3Acl, user: &str, required: &S3Permission) -> bool {
    // Simple check: owner has all permissions
    if let Some(ref owner) = acl.owner {
        if owner == user {
            return true;
        }
    }

    // Check grants
    for grant in &acl.grants {
        if grant.grantee == user {
            match (&grant.permission, required) {
                (S3Permission::FullControl, _) => return true,
                (perm, req) if perm == req => return true,
                _ => {}
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acl_serialization() {
        let acl = S3Acl {
            owner: Some("user123".to_string()),
            grants: vec![S3Grant {
                grantee: "user456".to_string(),
                permission: S3Permission::Read,
            }],
        };

        let json = acl.to_json().unwrap();
        let deserialized = S3Acl::from_json(&json).unwrap();

        assert_eq!(deserialized.owner, Some("user123".to_string()));
        assert_eq!(deserialized.grants.len(), 1);
        assert_eq!(deserialized.grants[0].permission, S3Permission::Read);
    }

    #[test]
    fn test_check_permission_owner() {
        let acl = S3Acl {
            owner: Some("alice".to_string()),
            grants: Vec::new(),
        };

        assert!(check_permission(&acl, "alice", &S3Permission::Read));
        assert!(check_permission(&acl, "alice", &S3Permission::Write));
        assert!(!check_permission(&acl, "bob", &S3Permission::Read));
    }

    #[test]
    fn test_check_permission_grant() {
        let acl = S3Acl {
            owner: Some("alice".to_string()),
            grants: vec![S3Grant {
                grantee: "bob".to_string(),
                permission: S3Permission::Read,
            }],
        };

        assert!(check_permission(&acl, "bob", &S3Permission::Read));
        assert!(!check_permission(&acl, "bob", &S3Permission::Write));
    }

    #[test]
    fn test_check_permission_full_control() {
        let acl = S3Acl {
            owner: Some("alice".to_string()),
            grants: vec![S3Grant {
                grantee: "bob".to_string(),
                permission: S3Permission::FullControl,
            }],
        };

        assert!(check_permission(&acl, "bob", &S3Permission::Read));
        assert!(check_permission(&acl, "bob", &S3Permission::Write));
        assert!(check_permission(&acl, "bob", &S3Permission::ReadAcp));
    }
}
