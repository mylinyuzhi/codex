//! Agent ignore service module.
//!
//! Provides shared infrastructure for handling agent-specific ignore files
//! (`.agentignore`, `.agentsignore`) along with standard `.gitignore` support.
//!
//! # Usage
//!
//! ```ignore
//! use codex_core::ignore_service::{AgentIgnoreService, IgnoreConfig};
//!
//! let service = AgentIgnoreService::with_defaults();
//! let walker = service.create_walk_builder(root_path);
//!
//! for entry in walker.build() {
//!     // Process files that pass ignore filters
//! }
//! ```

mod agent_ignore;
mod patterns;

pub use agent_ignore::AgentIgnoreService;
pub use agent_ignore::IgnoreConfig;
pub use patterns::BINARY_FILE_PATTERNS;
pub use patterns::COMMON_DIRECTORY_EXCLUDES;
pub use patterns::COMMON_IGNORE_PATTERNS;
pub use patterns::SYSTEM_FILE_EXCLUDES;
pub use patterns::get_all_default_excludes;
