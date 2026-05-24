//! cocode-tools - Builtin tool implementations for the agent system.
//!
//! This crate provides the 40+ builtin tools (Read, Write, Edit, Bash, etc.)
//! and the `register_builtin_tools()` function for populating a `ToolRegistry`.
//!
//! The Tool trait, ToolContext, ToolRegistry, and StreamingToolExecutor are
//! provided by `cocode-tools-api` and re-exported here for convenience.
//!
//! # Quick Start
//!
//! ```ignore
//! use cocode_tools::{ToolRegistry, StreamingToolExecutor, ExecutorConfig};
//! use cocode_tools::builtin::{register_builtin_tools, create_default_team_stores};
//! use std::sync::Arc;
//!
//! // Create and populate registry
//! let features = cocode_protocol::Features::with_defaults();
//! let (team_store, mailbox) = create_default_team_stores();
//! let mut registry = ToolRegistry::new();
//! register_builtin_tools(&mut registry, &features, team_store, mailbox);
//! ```

pub mod builtin;

// Re-export entire API surface from cocode-tools-api so that existing
// `use cocode_tools::Foo` and `use cocode_tools::module::Bar` paths
// continue to work without changes.
pub use cocode_tools_api::*;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::ConcurrencySafety;
    pub use crate::PermissionMode;
    pub use crate::ToolCall;
    pub use crate::ToolDefinition;
    pub use crate::builtin::builtin_tool_names;
    pub use crate::builtin::create_default_team_stores;
    pub use crate::builtin::register_builtin_tools;
    pub use crate::context::ToolContext;
    pub use crate::error::Result;
    pub use crate::error::ToolError;
    pub use crate::executor::ExecutorConfig;
    pub use crate::executor::StreamingToolExecutor;
    pub use crate::registry::ToolRegistry;
    pub use crate::tool::Tool;
    pub use crate::tool::ToolOutputExt;
}
