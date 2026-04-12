//! Shell execution, bash security, destructive warnings, sandbox decisions,
//! and shell environment snapshot support.
//!
//! HYBRID: cocode-rs utils/shell-parser (24 analyzers) as base + TS enhancements.
//! TS: utils/bash/ (12K LOC), utils/Shell.ts, tools/BashTool/ (23K total)

pub mod ast;
pub mod bash_permissions;
pub mod destructive;
pub mod executor;
pub mod heredoc;
pub mod mode_validation;
pub mod path_validation;
pub mod read_only;
pub mod result;
pub mod safety;
pub mod sandbox;
pub mod security;
pub mod sed_parser;
pub mod semantics;
pub mod shell_types;
pub mod shell_utils;
pub mod snapshot;
pub mod tokenizer;

pub use ast::BashNode;
pub use ast::SimpleCommand;
pub use ast::extract_simple_commands;
pub use ast::parse_command;
pub use bash_permissions::get_command_prefix;
pub use bash_permissions::is_dangerous_bare_prefix;
pub use bash_permissions::split_compound_command;
pub use bash_permissions::strip_all_env_vars;
pub use bash_permissions::strip_safe_wrappers;
pub use executor::ShellExecutor;
pub use executor::ShellProgress;
pub use heredoc::HeredocContent;
pub use heredoc::extract_heredocs;
pub use mode_validation::is_auto_allowed_in_accept_edits;
pub use result::CommandResult;
pub use result::ExecOptions;
pub use safety::SafetyResult;
pub use safety::SecurityCheckId;
pub use security::SecurityCheck;
pub use security::SecuritySeverity;
pub use security::check_security;
pub use sed_parser::SedEditInfo;
pub use sed_parser::is_sed_in_place_edit;
pub use sed_parser::parse_sed_edit_command;
pub use semantics::CommandResultInterpretation;
pub use semantics::interpret_command_result;
pub use shell_types::Shell;
pub use shell_types::ShellType;
pub use shell_types::default_user_shell;
pub use shell_types::detect_shell_type;
pub use shell_types::get_shell;
pub use snapshot::ShellSnapshot;
pub use snapshot::SnapshotConfig;
pub use snapshot::cleanup_stale_snapshots;
