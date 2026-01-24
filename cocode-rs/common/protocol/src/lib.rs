//! Protocol types for cocode multi-provider SDK.
//!
//! This crate provides the foundational types used across the cocode ecosystem:
//! - Model capabilities and reasoning levels
//! - Model configuration types
//! - Shell and truncation policies

pub mod model;

pub use model::Capability;
pub use model::ConfigShellToolType;
pub use model::ModelInfo;
pub use model::ReasoningEffort;
pub use model::TruncationMode;
pub use model::TruncationPolicyConfig;
pub use model::effort_rank;
pub use model::nearest_effort;
