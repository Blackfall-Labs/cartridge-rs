//! Condition evaluation for IAM policies
//!
//! Conditions allow fine-grained control based on context:
//! - String operations (Equals, Like, NotEquals)
//! - Numeric operations (Equals, LessThan, GreaterThan)
//! - Date operations (LessThan, GreaterThan)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Condition operator
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionOperator {
    /// String equals (case-sensitive)
    StringEquals,
    /// String matches pattern (supports * wildcard)
    StringLike,
    /// String not equals
    StringNotEquals,
    /// Numeric equals
    NumericEquals,
    /// Numeric less than
    NumericLessThan,
    /// Numeric less than or equals
    NumericLessThanEquals,
    /// Numeric greater than
    NumericGreaterThan,
    /// Numeric greater than or equals
    NumericGreaterThanEquals,
    /// Date less than (ISO 8601 format)
    DateLessThan,
    /// Date greater than (ISO 8601 format)
    DateGreaterThan,
}

/// Condition value (can be string, number, or date)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConditionValue {
    String(String),
    Number(f64),
    Bool(bool),
}

impl ConditionValue {
    fn as_string(&self) -> Option<&str> {
        match self {
            ConditionValue::String(s) => Some(s),
            _ => None,
        }
    }

    fn as_number(&self) -> Option<f64> {
        match self {
            ConditionValue::Number(n) => Some(*n),
            _ => None,
        }
    }
}

/// A single condition
#[derive(Debug, Clone)]
pub struct Condition {
    pub operator: ConditionOperator,
    pub key: String,
    pub value: ConditionValue,
}

impl Condition {
    /// Create a new condition
    pub fn new(operator: ConditionOperator, key: String, value: ConditionValue) -> Self {
        Condition {
            operator,
            key,
            value,
        }
    }

    /// Evaluate this condition against a context
    pub fn evaluate(&self, context: &HashMap<String, ConditionValue>) -> bool {
        let context_value = match context.get(&self.key) {
            Some(v) => v,
            None => return false, // Key not in context - condition fails
        };

        match &self.operator {
            ConditionOperator::StringEquals => {
                match (self.value.as_string(), context_value.as_string()) {
                    (Some(expected), Some(actual)) => expected == actual,
                    _ => false,
                }
            }
            ConditionOperator::StringLike => {
                match (self.value.as_string(), context_value.as_string()) {
                    (Some(pattern), Some(actual)) => Self::string_like(pattern, actual),
                    _ => false,
                }
            }
            ConditionOperator::StringNotEquals => {
                match (self.value.as_string(), context_value.as_string()) {
                    (Some(expected), Some(actual)) => expected != actual,
                    _ => false,
                }
            }
            ConditionOperator::NumericEquals => {
                match (self.value.as_number(), context_value.as_number()) {
                    (Some(expected), Some(actual)) => (expected - actual).abs() < f64::EPSILON,
                    _ => false,
                }
            }
            ConditionOperator::NumericLessThan => {
                match (self.value.as_number(), context_value.as_number()) {
                    (Some(expected), Some(actual)) => actual < expected,
                    _ => false,
                }
            }
            ConditionOperator::NumericLessThanEquals => {
                match (self.value.as_number(), context_value.as_number()) {
                    (Some(expected), Some(actual)) => actual <= expected,
                    _ => false,
                }
            }
            ConditionOperator::NumericGreaterThan => {
                match (self.value.as_number(), context_value.as_number()) {
                    (Some(expected), Some(actual)) => actual > expected,
                    _ => false,
                }
            }
            ConditionOperator::NumericGreaterThanEquals => {
                match (self.value.as_number(), context_value.as_number()) {
                    (Some(expected), Some(actual)) => actual >= expected,
                    _ => false,
                }
            }
            ConditionOperator::DateLessThan | ConditionOperator::DateGreaterThan => {
                // Simplified: treat as string comparison for ISO 8601 dates
                match (self.value.as_string(), context_value.as_string()) {
                    (Some(expected), Some(actual)) => {
                        if matches!(self.operator, ConditionOperator::DateLessThan) {
                            actual < expected
                        } else {
                            actual > expected
                        }
                    }
                    _ => false,
                }
            }
        }
    }

    /// String pattern matching with * wildcard
    fn string_like(pattern: &str, text: &str) -> bool {
        if !pattern.contains('*') {
            return pattern == text;
        }

        let parts: Vec<&str> = pattern.split('*').collect();

        // Must start with first part
        if !parts[0].is_empty() && !text.starts_with(parts[0]) {
            return false;
        }

        // Must end with last part
        if !parts[parts.len() - 1].is_empty() && !text.ends_with(parts[parts.len() - 1]) {
            return false;
        }

        // Check middle parts appear in order
        let mut pos = parts[0].len();
        for part in &parts[1..parts.len() - 1] {
            if part.is_empty() {
                continue;
            }
            match text[pos..].find(part) {
                Some(found) => pos += found + part.len(),
                None => return false,
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context(pairs: Vec<(&str, ConditionValue)>) -> HashMap<String, ConditionValue> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    #[test]
    fn test_string_equals() {
        let cond = Condition::new(
            ConditionOperator::StringEquals,
            "user".to_string(),
            ConditionValue::String("alice".to_string()),
        );

        let ctx = make_context(vec![("user", ConditionValue::String("alice".to_string()))]);
        assert!(cond.evaluate(&ctx));

        let ctx = make_context(vec![("user", ConditionValue::String("bob".to_string()))]);
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_string_like() {
        let cond = Condition::new(
            ConditionOperator::StringLike,
            "email".to_string(),
            ConditionValue::String("*@example.com".to_string()),
        );

        let ctx = make_context(vec![(
            "email",
            ConditionValue::String("alice@example.com".to_string()),
        )]);
        assert!(cond.evaluate(&ctx));

        let ctx = make_context(vec![(
            "email",
            ConditionValue::String("alice@other.com".to_string()),
        )]);
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_numeric_less_than() {
        let cond = Condition::new(
            ConditionOperator::NumericLessThan,
            "age".to_string(),
            ConditionValue::Number(18.0),
        );

        let ctx = make_context(vec![("age", ConditionValue::Number(16.0))]);
        assert!(cond.evaluate(&ctx));

        let ctx = make_context(vec![("age", ConditionValue::Number(20.0))]);
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_numeric_greater_than() {
        let cond = Condition::new(
            ConditionOperator::NumericGreaterThan,
            "score".to_string(),
            ConditionValue::Number(100.0),
        );

        let ctx = make_context(vec![("score", ConditionValue::Number(150.0))]);
        assert!(cond.evaluate(&ctx));

        let ctx = make_context(vec![("score", ConditionValue::Number(50.0))]);
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_date_comparison() {
        let cond = Condition::new(
            ConditionOperator::DateLessThan,
            "timestamp".to_string(),
            ConditionValue::String("2024-12-31".to_string()),
        );

        let ctx = make_context(vec![(
            "timestamp",
            ConditionValue::String("2024-01-01".to_string()),
        )]);
        assert!(cond.evaluate(&ctx));

        let ctx = make_context(vec![(
            "timestamp",
            ConditionValue::String("2025-01-01".to_string()),
        )]);
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_missing_context_key() {
        let cond = Condition::new(
            ConditionOperator::StringEquals,
            "user".to_string(),
            ConditionValue::String("alice".to_string()),
        );

        let ctx = HashMap::new();
        assert!(!cond.evaluate(&ctx)); // Missing key = condition fails
    }

    #[test]
    fn test_string_like_patterns() {
        assert!(Condition::string_like("test*", "testing"));
        assert!(Condition::string_like("*test", "unittest"));
        assert!(Condition::string_like("*test*", "testing123"));
        assert!(Condition::string_like("a*b*c", "abc"));
        assert!(Condition::string_like("a*b*c", "aXXbYYc"));
        assert!(!Condition::string_like("a*b*c", "acb"));
    }
}
