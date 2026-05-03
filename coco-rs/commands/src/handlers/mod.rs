//! Individual command handler implementations.
//!
//! Each sub-module owns one slash-command's full async logic (file I/O, git
//! commands, formatting). The parent `implementations.rs` wires these into
//! the `CommandRegistry` via `AsyncBuiltinCommand`.

pub mod agents;
pub mod btw;
pub mod clear;
pub mod compact;
pub mod context;
pub mod cost;
pub mod diff;
pub mod dream;
pub mod files;
pub mod help;
pub mod hooks;
pub mod init_prompt;
pub mod mcp;
pub mod memory;
pub mod memory_dialog;
pub mod migration;
pub mod model;
pub mod permissions;
pub mod plugin;
pub mod prompt_command;
pub mod rewind;
pub mod sentinel;
pub mod session;
pub mod summary;

pub mod stats;
