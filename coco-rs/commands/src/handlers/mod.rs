//! Individual command handler implementations.
//!
//! Each sub-module owns one slash-command's full async logic (file I/O, git
//! commands, formatting). The parent `implementations.rs` wires these into
//! the `CommandRegistry` via `AsyncBuiltinCommand`.

pub mod clear;
pub mod compact;
pub mod context;
pub mod cost;
pub mod diff;
pub mod files;
pub mod help;
pub mod hooks;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod permissions;
pub mod plugin;
pub mod session;

pub mod stats;
