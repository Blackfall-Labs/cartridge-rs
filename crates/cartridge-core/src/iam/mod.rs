//! Identity and Access Management (IAM) for Cartridge
//!
//! Provides fine-grained access control with:
//! - JSON-based policy documents
//! - Allow/Deny statements with explicit deny precedence
//! - Wildcard pattern matching for resources
//! - Condition evaluation (String, Numeric, Date operations)
//! - LRU caching for high-performance evaluation (10,000+ evals/sec)

mod cache;
mod condition;
mod engine;
mod pattern;
mod policy;

pub use cache::PolicyCache;
pub use condition::{Condition, ConditionOperator, ConditionValue};
pub use engine::PolicyEngine;
pub use pattern::PatternMatcher;
pub use policy::{Action, Effect, Policy, Statement};

#[cfg(test)]
mod tests;
