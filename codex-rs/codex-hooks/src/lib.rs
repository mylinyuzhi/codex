//! # Codex Hooks System
//!
//! A unified, extensible hook system for intercepting and controlling tool execution in codex-rs.
//!
//! ## Overview
//!
//! The hooks system provides synchronous interception points (hooks) at various stages of
//! the tool execution lifecycle. Hooks can:
//! - Block or allow operations
//! - Transform commands and environment
//! - Log and audit tool usage
//! - Implement custom security policies
//!
//! ## Architecture
//!
//! - **Protocol Layer**: Claude Code-compatible event and configuration formats
//! - **Action System**: Pluggable actions (Bash scripts, Rust functions, etc.)
//! - **Executor**: Manages sequential/parallel hook execution
//! - **Manager**: Global registry and trigger system
//!
//! ## Usage
//!
//! ### Registering Native Hooks
//!
//! ```rust,ignore
//! use codex_hooks::action::registry::register_native_hook;
//! use codex_hooks::decision::{HookResult, HookEffect};
//!
//! register_native_hook("my_security_check", |ctx| {
//!     // Implement your security logic
//!     if is_dangerous_command(&ctx) {
//!         HookResult::abort("Dangerous command detected")
//!     } else {
//!         HookResult::continue_with(vec![])
//!     }
//! });
//! ```
//!
//! ### Triggering Hooks
//!
//! ```rust,ignore
//! use codex_hooks::manager::trigger_hook;
//! use codex_protocol::hooks::{HookEventContext, HookEventName, HookEventData};
//!
//! let event = HookEventContext {
//!     session_id: "session-123".to_string(),
//!     hook_event_name: HookEventName::PreToolUse,
//!     // ... other fields
//! };
//!
//! trigger_hook(event).await?;
//! ```

pub mod action;
pub mod config;
pub mod context;
pub mod decision;
pub mod executor;
pub mod manager;
pub mod types;

// Re-export commonly used types
pub use action::{HookAction, HookActionError};
pub use context::{HookContext, HookState};
pub use decision::{HookDecision, HookEffect, HookResult};
pub use executor::{ExecutionResult, HookExecutor};
pub use manager::{trigger_hook, HookError, HookManager};
pub use types::{HookMetadata, HookPhase, HookPriority};
