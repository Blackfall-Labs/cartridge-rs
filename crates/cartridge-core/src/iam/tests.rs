//! Integration tests for IAM policy engine

use super::*;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_complex_policy_scenario() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    // Public read access
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read, Action::List],
        vec!["/public/**".to_string()],
    ));

    // Admin full access
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::All],
        vec!["/admin/**".to_string()],
    ));

    // User-specific access
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read, Action::Write],
        vec!["/users/*/documents/*".to_string()],
    ));

    // Deny sensitive data
    policy.add_statement(Statement::new(
        Effect::Deny,
        vec![Action::All],
        vec!["/admin/secrets/**".to_string()],
    ));

    // Public reads allowed
    assert!(engine.evaluate(&policy, &Action::Read, "/public/readme.txt", None));
    assert!(engine.evaluate(&policy, &Action::List, "/public/files", None));

    // Admin access allowed (except secrets)
    assert!(engine.evaluate(&policy, &Action::Read, "/admin/config.txt", None));
    assert!(engine.evaluate(&policy, &Action::Write, "/admin/config.txt", None));
    assert!(engine.evaluate(&policy, &Action::Delete, "/admin/old.txt", None));

    // Secrets denied (explicit deny overrides admin allow)
    assert!(!engine.evaluate(&policy, &Action::Read, "/admin/secrets/key.pem", None));
    assert!(!engine.evaluate(&policy, &Action::Write, "/admin/secrets/key.pem", None));

    // User documents allowed
    assert!(engine.evaluate(
        &policy,
        &Action::Read,
        "/users/alice/documents/file.txt",
        None
    ));
    assert!(engine.evaluate(
        &policy,
        &Action::Write,
        "/users/bob/documents/data.json",
        None
    ));

    // But not other user paths
    assert!(!engine.evaluate(
        &policy,
        &Action::Read,
        "/users/alice/settings/config.json",
        None
    ));
}

#[test]
fn test_policy_json_roundtrip_with_evaluation() {
    let policy_json = json!({
        "Version": "2024-01-01",
        "Statement": [
            {
                "Effect": "Allow",
                "Action": ["read", "list"],
                "Resource": ["/public/*"]
            },
            {
                "Effect": "Deny",
                "Action": ["write", "delete"],
                "Resource": ["/protected/**"]
            }
        ]
    });

    let policy: Policy = serde_json::from_value(policy_json).unwrap();
    assert_eq!(policy.statement.len(), 2);

    let mut engine = PolicyEngine::new_default();

    // Allow reads
    assert!(engine.evaluate(&policy, &Action::Read, "/public/file.txt", None));

    // Deny writes
    assert!(!engine.evaluate(&policy, &Action::Write, "/protected/data.json", None));
}

#[test]
fn test_cache_performance() {
    let mut engine = PolicyEngine::new(100);
    let mut policy = Policy::new();

    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec!["/data/**".to_string()],
    ));

    // First evaluation - populates cache
    for i in 0..50 {
        let path = format!("/data/file{}.txt", i);
        assert!(engine.evaluate(&policy, &Action::Read, &path, None));
    }

    assert_eq!(engine.cache_size(), 50);

    // Repeat evaluations - should use cache
    for i in 0..50 {
        let path = format!("/data/file{}.txt", i);
        assert!(engine.evaluate(&policy, &Action::Read, &path, None));
    }

    // Cache size should remain the same (LRU)
    assert!(engine.cache_size() <= 100);
}

#[test]
fn test_pattern_matching_edge_cases() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec![
            "/exact/path.txt".to_string(),
            "/single/*/wildcard".to_string(),
            "/recursive/**/wildcard".to_string(),
        ],
    ));

    // Exact match
    assert!(engine.evaluate(&policy, &Action::Read, "/exact/path.txt", None));
    assert!(!engine.evaluate(&policy, &Action::Read, "/exact/other.txt", None));

    // Single wildcard
    assert!(engine.evaluate(&policy, &Action::Read, "/single/anything/wildcard", None));
    assert!(!engine.evaluate(&policy, &Action::Read, "/single/a/b/wildcard", None));

    // Recursive wildcard
    assert!(engine.evaluate(&policy, &Action::Read, "/recursive/wildcard", None));
    assert!(engine.evaluate(&policy, &Action::Read, "/recursive/a/wildcard", None));
    assert!(engine.evaluate(&policy, &Action::Read, "/recursive/a/b/c/wildcard", None));
}

#[test]
fn test_action_wildcard_combinations() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    // Allow specific action
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec!["/specific/**".to_string()],
    ));

    // Allow all actions
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::All],
        vec!["/all/**".to_string()],
    ));

    // Specific path - only read
    assert!(engine.evaluate(&policy, &Action::Read, "/specific/file.txt", None));
    assert!(!engine.evaluate(&policy, &Action::Write, "/specific/file.txt", None));

    // All path - all actions
    assert!(engine.evaluate(&policy, &Action::Read, "/all/file.txt", None));
    assert!(engine.evaluate(&policy, &Action::Write, "/all/file.txt", None));
    assert!(engine.evaluate(&policy, &Action::Delete, "/all/file.txt", None));
    assert!(engine.evaluate(&policy, &Action::List, "/all", None));
    assert!(engine.evaluate(&policy, &Action::Create, "/all/new.txt", None));
}

#[test]
fn test_multiple_resource_patterns() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec![
            "/docs/**".to_string(),
            "/images/**".to_string(),
            "/videos/**".to_string(),
        ],
    ));

    // All specified paths allowed
    assert!(engine.evaluate(&policy, &Action::Read, "/docs/readme.md", None));
    assert!(engine.evaluate(&policy, &Action::Read, "/images/photo.jpg", None));
    assert!(engine.evaluate(&policy, &Action::Read, "/videos/clip.mp4", None));

    // Other paths denied
    assert!(!engine.evaluate(&policy, &Action::Read, "/audio/song.mp3", None));
}

#[test]
fn test_deny_specific_allow_general() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    // General allow
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::All],
        vec!["/**".to_string()],
    ));

    // Specific deny
    policy.add_statement(Statement::new(
        Effect::Deny,
        vec![Action::Delete],
        vec!["/important/**".to_string()],
    ));

    // Can read/write important files
    assert!(engine.evaluate(&policy, &Action::Read, "/important/data.txt", None));
    assert!(engine.evaluate(&policy, &Action::Write, "/important/data.txt", None));

    // Cannot delete important files
    assert!(!engine.evaluate(&policy, &Action::Delete, "/important/data.txt", None));

    // Can delete other files
    assert!(engine.evaluate(&policy, &Action::Delete, "/temp/data.txt", None));
}

#[test]
fn test_overlapping_patterns() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    // Broader pattern
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec!["/data/**".to_string()],
    ));

    // Narrower deny pattern
    policy.add_statement(Statement::new(
        Effect::Deny,
        vec![Action::Read],
        vec!["/data/sensitive/**".to_string()],
    ));

    // Broader pattern allows
    assert!(engine.evaluate(&policy, &Action::Read, "/data/public.txt", None));
    assert!(engine.evaluate(&policy, &Action::Read, "/data/reports/q1.txt", None));

    // Narrower deny overrides
    assert!(!engine.evaluate(&policy, &Action::Read, "/data/sensitive/secret.txt", None));
}

#[test]
fn test_root_path_access() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::List],
        vec!["/".to_string()],
    ));

    assert!(engine.evaluate(&policy, &Action::List, "/", None));
    assert!(!engine.evaluate(&policy, &Action::List, "/subdir", None));
}

#[test]
fn test_policy_validation_integration() {
    let mut policy = Policy::new();

    // Empty policy should fail validation
    assert!(policy.validate().is_err());

    // Add valid statement
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec!["/test".to_string()],
    ));

    assert!(policy.validate().is_ok());

    // Can evaluate
    let mut engine = PolicyEngine::new_default();
    assert!(engine.evaluate(&policy, &Action::Read, "/test", None));
}

#[test]
fn test_case_sensitivity() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec!["/Data/**".to_string()],
    ));

    // Exact case match
    assert!(engine.evaluate(&policy, &Action::Read, "/Data/file.txt", None));

    // Different case - should not match (paths are case-sensitive)
    assert!(!engine.evaluate(&policy, &Action::Read, "/data/file.txt", None));
}

#[test]
fn test_normalized_paths() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec!["/docs/*".to_string()],
    ));

    // Different path formats (normalization should handle)
    assert!(engine.evaluate(&policy, &Action::Read, "/docs/readme.md", None));
    assert!(engine.evaluate(&policy, &Action::Read, "docs/readme.md", None)); // No leading slash
}

#[test]
fn test_multiple_actions_per_statement() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read, Action::Write, Action::List],
        vec!["/workspace/**".to_string()],
    ));

    // All specified actions allowed
    assert!(engine.evaluate(&policy, &Action::Read, "/workspace/file.txt", None));
    assert!(engine.evaluate(&policy, &Action::Write, "/workspace/file.txt", None));
    assert!(engine.evaluate(&policy, &Action::List, "/workspace", None));

    // Other actions denied
    assert!(!engine.evaluate(&policy, &Action::Delete, "/workspace/file.txt", None));
    assert!(!engine.evaluate(&policy, &Action::Create, "/workspace/new.txt", None));
}

#[test]
fn test_statement_ordering() {
    let mut engine = PolicyEngine::new_default();
    let mut policy = Policy::new();

    // Order shouldn't matter for deny precedence
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec!["/**".to_string()],
    ));

    policy.add_statement(Statement::new(
        Effect::Deny,
        vec![Action::Read],
        vec!["/secret/**".to_string()],
    ));

    // Deny still overrides even though allow came first
    assert!(!engine.evaluate(&policy, &Action::Read, "/secret/key.txt", None));

    // Reverse order - same result
    let mut policy2 = Policy::new();

    policy2.add_statement(Statement::new(
        Effect::Deny,
        vec![Action::Read],
        vec!["/secret/**".to_string()],
    ));

    policy2.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read],
        vec!["/**".to_string()],
    ));

    let mut engine2 = PolicyEngine::new_default();
    assert!(!engine2.evaluate(&policy2, &Action::Read, "/secret/key.txt", None));
}
