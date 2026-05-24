//! IDE integration for cocode.
//!
//! This crate provides IDE extension connectivity via MCP protocol,
//! enabling features like live selection context, diagnostic tracking,
//! and IDE diff previews.

pub mod context;
pub mod detection;
pub mod diagnostics_manager;
pub mod diff_handler;
pub mod error;
pub mod lockfile;
pub mod mcp_bridge;
pub mod selection;
pub mod tool_filter;
pub mod wsl;

pub use context::IdeContext;
pub use detection::IDE_REGISTRY;
pub use detection::IdeKind;
pub use detection::IdeType;
pub use diagnostics_manager::IdeDiagnostic;
pub use diagnostics_manager::IdeDiagnosticsManager;
pub use diff_handler::DiffResult;
pub use diff_handler::IdeDiffHandler;
pub use error::Error;
pub use error::Result;
pub use mcp_bridge::ConnectionStatus;
pub use mcp_bridge::DiffResolution;
pub use mcp_bridge::IdeMcpBridge;
pub use selection::IdeSelection;
pub use selection::IdeSelectionState;
pub use tool_filter::should_expose_to_model;
