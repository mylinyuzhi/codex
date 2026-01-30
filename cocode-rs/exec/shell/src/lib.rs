//! Shell command execution for the cocode agent.
//!
//! This crate provides shell command execution with:
//! - Timeout support
//! - Output capture and truncation
//! - Background task management
//! - Read-only command detection
//! - Shell environment snapshotting
//! - CWD tracking and subagent isolation
//!
//! ## Shell Snapshotting
//!
//! Shell snapshotting captures the user's shell environment (aliases, functions,
//! exports, options) and restores them before each command execution. This ensures
//! commands run with the same environment as the user's interactive shell.
//!
//! Snapshotting is **enabled by default**. To disable, set the environment variable:
//! ```sh
//! export COCODE_DISABLE_SHELL_SNAPSHOT=1
//! ```
//!
//! ## CWD Tracking
//!
//! The executor can track working directory changes across commands:
//!
//! ```no_run
//! use cocode_shell::ShellExecutor;
//! use std::path::PathBuf;
//!
//! # async fn example() {
//! let mut executor = ShellExecutor::new(PathBuf::from("/project"));
//!
//! // Use execute_with_cwd_tracking to track cd changes
//! executor.execute_with_cwd_tracking("cd src", 10).await;
//!
//! // Subsequent commands use the new CWD
//! assert!(executor.cwd().ends_with("src"));
//! # }
//! ```
//!
//! ## Subagent Shell Execution
//!
//! For subagent scenarios (parallel task agents), use `fork_for_subagent()`:
//!
//! ```no_run
//! use cocode_shell::ShellExecutor;
//! use std::path::PathBuf;
//!
//! # async fn example() {
//! // Main agent executor
//! let main_executor = ShellExecutor::with_default_shell(PathBuf::from("/project"));
//!
//! // Fork for subagent - uses initial CWD, no CWD tracking
//! let subagent_executor = main_executor.fork_for_subagent(PathBuf::from("/project"));
//!
//! // Subagent bash calls always start from initial CWD
//! subagent_executor.execute("cd /tmp && pwd", 10).await;  // outputs /tmp
//! subagent_executor.execute("pwd", 10).await;             // outputs /project (reset!)
//! # }
//! ```
//!
//! **Important**: Subagents should use absolute paths since CWD resets between calls.
//!
//! The forked executor:
//! - Uses the provided initial CWD (not the main executor's current CWD)
//! - Shares the shell snapshot (read-only)
//! - Has its own independent background task registry
//! - Does NOT track CWD changes between calls

pub mod background;
pub mod command;
pub mod executor;
pub mod readonly;
pub mod shell_types;
pub mod snapshot;

pub use background::{BackgroundProcess, BackgroundTaskRegistry};
pub use command::{CommandInput, CommandResult};
pub use executor::ShellExecutor;
pub use readonly::{is_git_read_only, is_read_only_command};
pub use shell_types::{
    Shell, ShellType, default_user_shell, detect_shell_type, get_shell, get_shell_by_path,
};
pub use snapshot::{ShellSnapshot, SnapshotConfig, cleanup_stale_snapshots};
