//! IAM Policy document structure
//!
//! Policies define what actions are allowed or denied on resources.
//! Format is inspired by AWS IAM policies but simplified for Cartridge use.

use serde::{Deserialize, Serialize};

/// Effect of a policy statement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    /// Allow the action
    Allow,
    /// Deny the action (takes precedence over Allow)
    Deny,
}

/// Actions that can be performed on cartridge resources
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Read file contents
    Read,
    /// Write or modify file contents
    Write,
    /// Delete files
    Delete,
    /// List directory contents
    List,
    /// Create new files or directories
    Create,
    /// All actions (wildcard)
    #[serde(rename = "*")]
    All,
}

impl Action {
    /// Check if this action matches another (considering wildcards)
    pub fn matches(&self, other: &Action) -> bool {
        match (self, other) {
            (Action::All, _) => true,
            (_, Action::All) => true,
            (a, b) => a == b,
        }
    }
}

/// A single policy statement
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Statement {
    /// Statement ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,

    /// Effect of this statement
    pub effect: Effect,

    /// Actions this statement applies to
    pub action: Vec<Action>,

    /// Resources this statement applies to (supports wildcards)
    pub resource: Vec<String>,

    /// Optional conditions for when this statement applies
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<serde_json::Value>,
}

impl Statement {
    /// Create a new statement
    pub fn new(effect: Effect, action: Vec<Action>, resource: Vec<String>) -> Self {
        Statement {
            sid: None,
            effect,
            action,
            resource,
            condition: None,
        }
    }

    /// Check if this statement applies to the given action and resource
    pub fn applies_to(&self, action: &Action, resource: &str) -> bool {
        // Check if action matches
        let action_matches = self.action.iter().any(|a| a.matches(action));
        if !action_matches {
            return false;
        }

        // Check if resource matches (will use pattern matcher later)
        self.resource
            .iter()
            .any(|pattern| crate::iam::PatternMatcher::matches(pattern, resource))
    }
}

/// Complete IAM policy document
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Policy {
    /// Policy format version
    pub version: String,

    /// List of policy statements
    pub statement: Vec<Statement>,
}

impl Policy {
    /// Create a new empty policy
    pub fn new() -> Self {
        Policy {
            version: "2024-01-01".to_string(),
            statement: Vec::new(),
        }
    }

    /// Add a statement to this policy
    pub fn add_statement(&mut self, statement: Statement) {
        self.statement.push(statement);
    }

    /// Parse policy from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize policy to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Validate policy structure
    pub fn validate(&self) -> Result<(), String> {
        if self.statement.is_empty() {
            return Err("Policy must have at least one statement".to_string());
        }

        for (i, stmt) in self.statement.iter().enumerate() {
            if stmt.action.is_empty() {
                return Err(format!("Statement {} has no actions", i));
            }
            if stmt.resource.is_empty() {
                return Err(format!("Statement {} has no resources", i));
            }
        }

        Ok(())
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_matches() {
        assert!(Action::All.matches(&Action::Read));
        assert!(Action::Read.matches(&Action::All));
        assert!(Action::Read.matches(&Action::Read));
        assert!(!Action::Read.matches(&Action::Write));
    }

    #[test]
    fn test_policy_creation() {
        let mut policy = Policy::new();
        assert_eq!(policy.statement.len(), 0);

        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read],
            vec!["/public/*".to_string()],
        ));

        assert_eq!(policy.statement.len(), 1);
    }

    #[test]
    fn test_policy_json_roundtrip() {
        let mut policy = Policy::new();
        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read, Action::List],
            vec!["/public/*".to_string()],
        ));

        let json = policy.to_json().unwrap();
        let parsed = Policy::from_json(&json).unwrap();

        assert_eq!(parsed.statement.len(), 1);
        assert_eq!(parsed.statement[0].effect, Effect::Allow);
        assert_eq!(parsed.statement[0].action.len(), 2);
    }

    #[test]
    fn test_policy_validation() {
        let empty_policy = Policy::new();
        assert!(empty_policy.validate().is_err());

        let mut valid_policy = Policy::new();
        valid_policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read],
            vec!["/test".to_string()],
        ));
        assert!(valid_policy.validate().is_ok());
    }

    #[test]
    fn test_statement_applies_to() {
        let stmt = Statement::new(
            Effect::Allow,
            vec![Action::Read],
            vec!["/public/*".to_string()],
        );

        assert!(stmt.applies_to(&Action::Read, "/public/file.txt"));
        assert!(!stmt.applies_to(&Action::Write, "/public/file.txt"));
    }
}
