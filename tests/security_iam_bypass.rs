//! IAM security tests - bypass attempts and edge cases
//!
//! Note: These tests use the internal core::Cartridge API since IAM features
//! are not yet exposed on the public wrapper.

use cartridge_rs::core::cartridge::Cartridge;
use cartridge_rs::core::iam::{Action, Effect, Policy, Statement};

#[test]
fn test_iam_path_traversal_attempts() {
    let mut cart = Cartridge::create("iam-traversal", "IAM Traversal").unwrap();

    // Create test files BEFORE setting policy
    cart.create_file("/public/file.txt", b"public data").unwrap();
    cart.create_file("/private/secret.txt", b"secret data").unwrap();

    // Policy: allow /public/*, deny /private/*
    let policy = Policy {
        version: "2012-10-17".to_string(),
        statement: vec![
            Statement::new(
                Effect::Allow,
                vec![Action::Read],
                vec!["/public/**".to_string()],
            ),
            Statement::new(
                Effect::Deny,
                vec![Action::Read],
                vec!["/private/**".to_string()],
            ),
        ],
    };
    cart.set_policy(policy);

    // Valid access
    assert!(cart.check_access(&Action::Read, "/public/file.txt").is_ok());

    // Direct denial
    assert!(cart.check_access(&Action::Read, "/private/secret.txt").is_err());

    // Path traversal attempts should fail
    assert!(cart.check_access(&Action::Read, "/public/../private/secret.txt").is_err());
    assert!(cart.check_access(&Action::Read, "/public/./../private/secret.txt").is_err());
    assert!(cart.check_access(&Action::Read, "/public/../../private/secret.txt").is_err());

    std::fs::remove_file("iam-traversal.cart").ok();
}

#[test]
fn test_iam_wildcard_semantics() {
    let mut cart = Cartridge::create("iam-wildcard", "IAM Wildcard").unwrap();

    // Create test files BEFORE setting policy
    cart.create_file("/data/file.txt", b"data").unwrap();
    cart.create_file("/data/subdir/file.txt", b"nested").unwrap();

    // Policy: allow /data/*.txt (single-level wildcard)
    let policy = Policy {
        version: "2012-10-17".to_string(),
        statement: vec![
            Statement::new(
                Effect::Allow,
                vec![Action::Read],
                vec!["/data/*.txt".to_string()],
            ),
        ],
    };
    cart.set_policy(policy);

    // Single-level wildcard should match direct children
    assert!(cart.check_access(&Action::Read, "/data/file.txt").is_ok());

    // Should NOT match nested paths (single * vs **)
    assert!(cart.check_access(&Action::Read, "/data/subdir/file.txt").is_err());

    std::fs::remove_file("iam-wildcard.cart").ok();
}

#[test]
fn test_iam_recursive_wildcard() {
    let mut cart = Cartridge::create("iam-recursive", "IAM Recursive").unwrap();

    // Create nested structure BEFORE setting policy
    cart.create_file("/data/file.txt", b"root").unwrap();
    cart.create_file("/data/level1/file.txt", b"l1").unwrap();
    cart.create_file("/data/level1/level2/file.txt", b"l2").unwrap();

    // Policy: allow /data/** (recursive wildcard)
    let policy = Policy {
        version: "2012-10-17".to_string(),
        statement: vec![
            Statement::new(
                Effect::Allow,
                vec![Action::Read],
                vec!["/data/**".to_string()],
            ),
        ],
    };
    cart.set_policy(policy);

    // Recursive wildcard should match all levels
    assert!(cart.check_access(&Action::Read, "/data/file.txt").is_ok());
    assert!(cart.check_access(&Action::Read, "/data/level1/file.txt").is_ok());
    assert!(cart.check_access(&Action::Read, "/data/level1/level2/file.txt").is_ok());

    std::fs::remove_file("iam-recursive.cart").ok();
}

#[test]
fn test_iam_deny_precedence() {
    let mut cart = Cartridge::create("iam-deny-prec", "IAM Deny Precedence").unwrap();

    // Create test files BEFORE setting policy
    cart.create_file("/public.txt", b"public").unwrap();
    cart.create_file("/secret.txt", b"secret").unwrap();

    // Policy: allow /**, but deny /secret.txt
    let policy = Policy {
        version: "2012-10-17".to_string(),
        statement: vec![
            Statement::new(
                Effect::Allow,
                vec![Action::Read, Action::Write],
                vec!["/**".to_string()],
            ),
            Statement::new(
                Effect::Deny,
                vec![Action::Read, Action::Write],
                vec!["/secret.txt".to_string()],
            ),
        ],
    };
    cart.set_policy(policy);

    // Allow should work for public files
    assert!(cart.check_access(&Action::Read, "/public.txt").is_ok());
    assert!(cart.check_access(&Action::Write, "/public.txt").is_ok());

    // Deny should override allow for secret.txt
    assert!(cart.check_access(&Action::Read, "/secret.txt").is_err());
    assert!(cart.check_access(&Action::Write, "/secret.txt").is_err());

    std::fs::remove_file("iam-deny-prec.cart").ok();
}

#[test]
fn test_iam_action_specificity() {
    let mut cart = Cartridge::create("iam-actions", "IAM Actions").unwrap();

    // Create test files BEFORE setting policy
    cart.create_file("/file.txt", b"data").unwrap();
    cart.create_file("/protected/file.txt", b"protected").unwrap();

    // Policy: allow Read, deny Write
    let policy = Policy {
        version: "2012-10-17".to_string(),
        statement: vec![
            Statement::new(
                Effect::Allow,
                vec![Action::Read],
                vec!["/**".to_string()],
            ),
            Statement::new(
                Effect::Deny,
                vec![Action::Write],
                vec!["/protected/**".to_string()],
            ),
        ],
    };
    cart.set_policy(policy);

    // Read should work everywhere
    assert!(cart.check_access(&Action::Read, "/file.txt").is_ok());
    assert!(cart.check_access(&Action::Read, "/protected/file.txt").is_ok());

    // Write should be denied for /protected/**
    assert!(cart.check_access(&Action::Write, "/protected/file.txt").is_err());

    // Write should be implicitly denied for /file.txt (no allow statement)
    assert!(cart.check_access(&Action::Write, "/file.txt").is_err());

    std::fs::remove_file("iam-actions.cart").ok();
}

#[test]
fn test_iam_empty_policy() {
    let mut cart = Cartridge::create("iam-empty", "IAM Empty").unwrap();

    // Create test file BEFORE setting policy
    cart.create_file("/file.txt", b"data").unwrap();

    // Empty policy should deny everything
    let policy = Policy {
        version: "2012-10-17".to_string(),
        statement: vec![],
    };
    cart.set_policy(policy);

    // All actions should be denied with empty policy
    assert!(cart.check_access(&Action::Read, "/file.txt").is_err());
    assert!(cart.check_access(&Action::Write, "/file.txt").is_err());
    assert!(cart.check_access(&Action::Delete, "/file.txt").is_err());

    std::fs::remove_file("iam-empty.cart").ok();
}

#[test]
fn test_iam_special_characters_in_paths() {
    let mut cart = Cartridge::create("iam-special", "IAM Special").unwrap();

    // Create test files with special characters BEFORE setting policy
    cart.create_file("/file with spaces.txt", b"data").unwrap();
    cart.create_file("/file-with-dashes.txt", b"data").unwrap();
    cart.create_file("/file_with_underscores.txt", b"data").unwrap();

    let policy = Policy {
        version: "2012-10-17".to_string(),
        statement: vec![
            Statement::new(
                Effect::Allow,
                vec![Action::Read],
                vec!["/**".to_string()],
            ),
        ],
    };
    cart.set_policy(policy);

    assert!(cart.check_access(&Action::Read, "/file with spaces.txt").is_ok());
    assert!(cart.check_access(&Action::Read, "/file-with-dashes.txt").is_ok());
    assert!(cart.check_access(&Action::Read, "/file_with_underscores.txt").is_ok());

    std::fs::remove_file("iam-special.cart").ok();
}

#[test]
fn test_iam_overlapping_patterns() {
    let mut cart = Cartridge::create("iam-overlap", "IAM Overlap").unwrap();

    // Create test files BEFORE setting policy
    cart.create_file("/data/file.txt", b"data").unwrap();
    cart.create_file("/data/restricted/secret.txt", b"secret").unwrap();
    cart.create_file("/data/restricted/public/file.txt", b"public").unwrap();

    // Multiple overlapping patterns
    let policy = Policy {
        version: "2012-10-17".to_string(),
        statement: vec![
            Statement::new(
                Effect::Allow,
                vec![Action::Read],
                vec!["/data/**".to_string()],
            ),
            Statement::new(
                Effect::Deny,
                vec![Action::Read],
                vec!["/data/restricted/**".to_string()],
            ),
            Statement::new(
                Effect::Allow,
                vec![Action::Read],
                vec!["/data/restricted/public/**".to_string()],
            ),
        ],
    };
    cart.set_policy(policy);

    // First level allowed
    assert!(cart.check_access(&Action::Read, "/data/file.txt").is_ok());

    // Restricted denied (deny takes precedence)
    assert!(cart.check_access(&Action::Read, "/data/restricted/secret.txt").is_err());

    // Even with allow on /data/restricted/public/**, deny on /data/restricted/** takes precedence
    assert!(cart.check_access(&Action::Read, "/data/restricted/public/file.txt").is_err());

    std::fs::remove_file("iam-overlap.cart").ok();
}
