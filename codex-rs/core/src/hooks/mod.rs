//! Hook system integration
//!
//! This module provides integration between the hook system and core.

pub mod integration;

// Re-export commonly used types
pub use codex_hooks::{trigger_hook, HookError};
