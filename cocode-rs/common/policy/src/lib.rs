//! Permission policy engine for tool execution control.
//!
//! This crate centralizes permission rule management:
//! - Rule types and pattern matching
//! - Rule evaluation with priority ordering
//! - Session-level approval storage
//! - Persistence to settings files

pub mod evaluator;
pub mod normalize;
pub mod persist;
pub mod rule;
pub mod store;

// Re-export primary types at crate root for convenience.
pub use evaluator::PermissionRuleEvaluator;
pub use normalize::normalize_command;
pub use persist::RuleDestination;
pub use persist::persist_rule;
pub use persist::persist_rule_with_options;
pub use persist::remove_rule;
pub use persist::remove_rule_with_options;
pub use rule::PermissionRule;
pub use rule::RuleAction;
pub use store::ApprovalStore;
