//! Plugin system for cocode-rs.
//!
//! This crate implements a plugin system that allows extending cocode with:
//! - Custom skills (slash commands)
//! - Custom hooks (lifecycle interceptors)
//! - Custom agents (specialized subagents)
//! - Custom commands (plugin-provided commands)
//! - MCP servers (Model Context Protocol servers)
//!
//! # Architecture
//!
//! Plugins are discovered from multiple scopes in priority order:
//! 1. **Flag** - `--plugin-dir` or inline plugins (highest priority)
//! 2. **Local** - Development/local plugins
//! 3. **Project** - `.cocode/plugins/` in the project directory
//! 4. **User** - `~/.cocode/plugins/`
//! 5. **Managed** - System-installed plugins (lowest priority)
//!
//! Each plugin contains a `plugin.json` manifest that declares its contributions.
//!
//! # Plugin Manifest
//!
//! ```json
//! {
//!   "plugin": {
//!     "name": "my-plugin",
//!     "version": "0.1.0",
//!     "description": "My custom plugin"
//!   },
//!   "contributions": {
//!     "skills": ["skills/"],
//!     "hooks": ["hooks.json"],
//!     "agents": ["agents/"],
//!     "commands": ["commands/"],
//!     "mcp_servers": ["mcp/"]
//!   }
//! }
//! ```

pub mod agent_loader;
pub mod cache;
pub mod command;
pub mod command_loader;
pub mod contribution;
pub mod dir_scanner;
pub mod git_clone;
pub mod installed_registry;
pub mod installer;
pub mod integration;
pub mod loader;
pub mod lsp_loader;
pub mod manifest;
pub mod marketplace_manager;
pub mod marketplace_types;
pub mod mcp;
pub mod mcp_loader;
pub mod plugin_settings;
pub mod policy;
pub mod registry;
pub mod scope;

mod error;

// Re-export primary types
pub use cache::DEFAULT_CACHE_GRACE_PERIOD;
pub use cache::cleanup_orphaned_cache;
pub use cache::plugins_dir;
pub use command::CommandHandler;
pub use command::PluginCommand;
pub use contribution::OutputStyleDefinition;
pub use contribution::PluginContribution;
pub use contribution::PluginContributions;
pub use contribution::StringOrVec;
pub use error::PluginError;
pub use error::Result;
pub use installed_registry::InstalledPluginsRegistry;
pub use installer::PluginInstaller;
pub use integration::ExtraMarketplaceEntry;
pub use integration::PluginIntegrationConfig;
pub use integration::PluginIntegrationResult;
pub use integration::connect_plugin_lsp_servers;
pub use integration::connect_plugin_mcp_servers;
pub use integration::integrate_plugins;
pub use integration::load_plugins;
pub use loader::LoadedPlugin;
pub use loader::PluginLoader;
pub use loader::load_plugins_from_roots;
pub use lsp_loader::LspServerConfig;
pub use manifest::PluginManifest;
pub use manifest::PluginRootSettings;
pub use manifest::UserConfigField;
pub use marketplace_manager::MarketplaceManager;
pub use marketplace_types::KnownMarketplace;
pub use marketplace_types::MarketplaceManifest;
pub use marketplace_types::MarketplacePluginEntry;
pub use marketplace_types::MarketplacePluginSource;
pub use marketplace_types::MarketplaceSource;
pub use mcp::McpServerConfig;
pub use mcp::McpTransport;
pub use plugin_settings::PluginSettings;
pub use policy::PluginPolicy;
pub use policy::PolicyDecision;
pub use registry::PluginRegistry;
pub use scope::PluginScope;
