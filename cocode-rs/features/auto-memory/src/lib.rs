//! Auto memory for cocode-rs.
//!
//! Provides persistent, cross-session knowledge storage through a
//! per-project `MEMORY.md` file and topic files. The `MEMORY.md` index
//! is loaded into the system prompt every turn (max 200 lines), and
//! agents can read/write topic files via standard tools.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                       cocode-auto-memory                            │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  config            │  directory         │  memory_file              │
//! │  - resolve config  │  - resolve dir     │  - load_memory_index()    │
//! │  - enable/disable  │  - ensure exists   │  - list_memory_files()    │
//! │  - env var chain   │  - project_hash()  │  - truncate_content()     │
//! ├────────────────────┼────────────────────┼───────────────────────────┤
//! │  path_check        │  staleness         │  prompt                   │
//! │  - is_auto_memory  │  - relative_time() │  - build_auto_memory_     │
//! │    _path()         │  - staleness_warn  │    prompt()               │
//! │                    │                    │  - build_background_      │
//! │                    │                    │    agent_memory_prompt()   │
//! ├────────────────────┴────────────────────┴───────────────────────────┤
//! │  state                                                              │
//! │  - AutoMemoryState (thread-safe, per-turn refresh)                  │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```

mod error;

pub mod config;
pub mod directory;
pub mod memory_file;
pub mod path_check;
pub mod prompt;
pub mod staleness;
pub mod state;

// Re-export primary types
pub use config::DisableReason;
pub use config::ResolvedAutoMemoryConfig;
pub use config::resolve_auto_memory_config;
pub use directory::TEAM_MEMORY_SUBDIR;
pub use directory::ensure_memory_dir_exists;
pub use directory::get_auto_memory_directory;
pub use directory::get_team_memory_directory;
pub use directory::project_hash;
pub use error::AutoMemoryError;
pub use error::Result;
pub use memory_file::AutoMemoryEntry;
pub use memory_file::MemoryFrontmatter;
pub use memory_file::MemoryIndex;
pub use memory_file::list_memory_files;
pub use memory_file::load_memory_file;
pub use memory_file::load_memory_index;
pub use memory_file::strip_html_comments;
pub use memory_file::truncate_content;
pub use path_check::PathTraversalError;
pub use path_check::is_auto_memory_path;
pub use path_check::is_team_memory_path;
pub use path_check::validate_team_memory_write_path;
pub use prompt::build_auto_memory_prompt;
pub use prompt::build_background_agent_memory_prompt;
pub use prompt::build_extract_mode_typed_combined_prompt;
pub use prompt::build_extraction_prompt_standard;
pub use prompt::build_extraction_prompt_team;
pub use prompt::build_extraction_prompt_typed;
pub use prompt::build_extraction_prompt_typed_team;
pub use prompt::build_typed_combined_memory_prompt;
pub use staleness::StalenessInfo;
pub use staleness::build_staleness_warning;
pub use staleness::format_relative_time;
pub use staleness::staleness_info;
pub use state::AutoMemoryState;
