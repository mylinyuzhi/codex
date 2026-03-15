//! LSP client library for cocode-rs
//!
//! Provides AI-friendly LSP operations with symbol name resolution
//! instead of exact line/column positions.
//!
//! # Supported Languages
//!
//! - Rust (rust-analyzer)
//! - Go (gopls)
//! - Python (pyright)
//! - TypeScript/JavaScript (typescript-language-server)
//!
//! # Example
//!
//! ```ignore
//! use cocode_lsp::{LspServersConfig, LspServerManager, DiagnosticsStore, SymbolKind};
//! use std::sync::Arc;
//!
//! let diagnostics = Arc::new(DiagnosticsStore::new());
//! let config = LspServersConfig::default();
//! let manager = LspServerManager::new(config, None, None, diagnostics);
//!
//! // Get client for a Rust file
//! let client = manager.get_client(Path::new("src/lib.rs")).await?;
//!
//! // Find definition using symbol name (AI-friendly)
//! let locations = client.definition(
//!     Path::new("src/lib.rs"),
//!     "Config",
//!     Some(SymbolKind::Struct)
//! ).await?;
//! ```

mod client;
mod server;

pub mod config;
pub mod diagnostics;
pub mod error;
pub mod installer;
pub mod lifecycle;
pub mod protocol;
pub mod symbols;

// Public exports
pub use client::LspClient;
pub use config::BUILTIN_SERVERS;
pub use config::BuiltinServer;
pub use config::ConfigLevel;
pub use config::LSP_SERVERS_CONFIG_FILE;
pub use config::LifecycleConfig;
pub use config::LspServerConfig;
pub use config::LspServersConfig;
pub use config::command_exists;
pub use diagnostics::DiagnosticEntry;
pub use diagnostics::DiagnosticSeverityLevel;
pub use diagnostics::DiagnosticsStore;
pub use error::LspErr;
pub use error::Result;
pub use installer::InstallEvent;
pub use installer::InstallerType;
pub use installer::LspInstaller;
pub use lifecycle::ServerHealth;
pub use lifecycle::ServerLifecycle;
pub use lifecycle::ServerStats;
pub use protocol::TimeoutConfig;
pub use server::LspServerManager;
pub use server::ServerConfigInfo;
pub use server::ServerStatus;
pub use server::ServerStatusInfo;
pub use symbols::ResolvedSymbol;
pub use symbols::SymbolKind;
pub use symbols::SymbolMatch;
pub use symbols::find_matching_symbols;
pub use symbols::flatten_symbols;

use std::path::PathBuf;
use std::sync::Arc;

/// Resolve the cocode home directory.
///
/// Standalone helper for binary entry points.
/// Library APIs (`LspServerManager::new`, `LspServersConfig::load`) accept
/// `cocode_home` as a parameter instead — callers are responsible for resolving it.
///
/// Checks `COCODE_HOME` env var first, falls back to `~/.cocode`.
/// This is a standalone implementation to avoid depending on `cocode-config`.
pub fn find_cocode_home() -> PathBuf {
    std::env::var("COCODE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cocode")
        })
}

/// Create an `LspServerManager` with standard configuration.
///
/// This is a convenience function that loads config from the standard locations
/// (`~/.cocode/lsp_servers.json` and `.cocode/lsp_servers.json`) and creates
/// the manager with a fresh diagnostics store.
///
/// # Arguments
/// * `cocode_home` - Cocode home directory for user-level config lookup.
/// * `cwd` - Working directory for the LSP servers. Used for project-local config
///           and as the workspace root.
///
/// # Example
/// ```ignore
/// use cocode_lsp::create_manager;
///
/// // Create manager for current directory
/// let manager = create_manager(None, Some(std::env::current_dir().unwrap()));
/// ```
pub fn create_manager(
    cocode_home: Option<&std::path::Path>,
    cwd: Option<PathBuf>,
) -> Arc<LspServerManager> {
    let lsp_config = LspServersConfig::load(cocode_home, cwd.as_deref());
    let diagnostics = Arc::new(DiagnosticsStore::new());
    Arc::new(LspServerManager::new(
        lsp_config,
        cocode_home.map(std::path::Path::to_path_buf),
        cwd,
        diagnostics,
    ))
}

// Re-export lsp_types for handler use
pub use lsp_types::CallHierarchyIncomingCall;
pub use lsp_types::CallHierarchyItem;
pub use lsp_types::CallHierarchyOutgoingCall;
pub use lsp_types::Location;
pub use lsp_types::SymbolInformation;

/// Re-export lsp_types for test use in other crates
pub mod lsp_types_reexport {
    pub use lsp_types::Position;
    pub use lsp_types::Range;
    pub use lsp_types::SymbolKind as LspSymbolKind;
    pub use lsp_types::Url;
}
