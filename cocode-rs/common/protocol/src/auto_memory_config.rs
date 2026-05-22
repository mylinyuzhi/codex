//! Auto memory configuration types.
//!
//! Controls the persistent, cross-session memory system that stores
//! knowledge in per-project `MEMORY.md` files.

use serde::Deserialize;
use serde::Serialize;

// ========================================================================
// Default Constants
// ========================================================================

/// Maximum lines loaded from MEMORY.md into system prompt.
pub const DEFAULT_MAX_MEMORY_LINES: i32 = 200;

/// Maximum number of relevant memory files returned per turn.
pub const DEFAULT_MAX_RELEVANT_FILES: i32 = 5;

/// Maximum lines per relevant memory file.
pub const DEFAULT_MAX_LINES_PER_FILE: i32 = 200;

/// Timeout in milliseconds for relevant memories search.
pub const DEFAULT_RELEVANT_SEARCH_TIMEOUT_MS: i64 = 5000;

/// Maximum lines to scan for YAML frontmatter in a memory file.
pub const DEFAULT_MAX_FRONTMATTER_LINES: i32 = 20;

/// Days after which a memory triggers a staleness warning.
pub const DEFAULT_STALENESS_WARNING_DAYS: i32 = 1;

/// Minimum turns between relevant memories generation.
pub const DEFAULT_RELEVANT_MEMORIES_THROTTLE_TURNS: i32 = 3;

/// Maximum memory files to scan when searching for relevant memories.
pub const DEFAULT_MAX_FILES_TO_SCAN: i32 = 200;

/// Minimum word length for keyword relevance matching.
pub const DEFAULT_MIN_KEYWORD_LENGTH: i32 = 3;

/// Default model role for LLM-based memory selection.
pub const DEFAULT_MEMORY_SELECTION_MODEL_ROLE: &str = "fast";

/// Maximum character count for MEMORY.md soft limit (TUI warning).
pub const DEFAULT_MAX_MEMORY_CHARS: i64 = 40_000;

/// Auto memory configuration.
///
/// # JSON Format
///
/// ```json
/// {
///   "autoMemory": {
///     "enabled": true,
///     "directory": "/custom/memory/path"
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AutoMemoryConfig {
    // ========================================================================
    // Feature Toggle
    // ========================================================================
    /// Whether auto memory is enabled.
    ///
    /// Can be overridden by `COCODE_DISABLE_AUTO_MEMORY` environment variable.
    #[serde(default)]
    pub enabled: Option<bool>,

    /// Custom memory directory path.
    ///
    /// Overrides the default `{home}/.cocode/projects/{hash}/memory/` path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,

    // ========================================================================
    // MEMORY.md Loading
    // ========================================================================
    /// Maximum lines loaded from MEMORY.md.
    #[serde(default = "default_max_lines")]
    pub max_lines: i32,

    /// Maximum lines to scan for YAML frontmatter in memory files.
    #[serde(default = "default_max_frontmatter_lines")]
    pub max_frontmatter_lines: i32,

    // ========================================================================
    // Relevant Memories Search
    // ========================================================================
    /// Maximum relevant memory files per turn.
    #[serde(default = "default_max_files")]
    pub max_relevant_files: i32,

    /// Maximum lines per relevant memory file.
    #[serde(default = "default_max_lines_per_file")]
    pub max_lines_per_file: i32,

    /// Timeout in milliseconds for relevant memories search.
    #[serde(default = "default_search_timeout")]
    pub relevant_search_timeout_ms: i64,

    /// Maximum memory files to scan when searching for relevant memories.
    #[serde(default = "default_max_files_to_scan")]
    pub max_files_to_scan: i32,

    /// Minimum turns between relevant memories generation.
    #[serde(default = "default_relevant_memories_throttle_turns")]
    pub relevant_memories_throttle_turns: i32,

    /// Minimum word length for keyword relevance matching.
    #[serde(default = "default_min_keyword_length")]
    pub min_keyword_length: i32,

    // ========================================================================
    // Staleness
    // ========================================================================
    /// Days after which a memory triggers a staleness warning.
    #[serde(default = "default_staleness_warning_days")]
    pub staleness_warning_days: i32,

    // ========================================================================
    // Character Limit (soft)
    // ========================================================================
    /// Maximum characters for MEMORY.md soft limit (TUI warning, no truncation).
    #[serde(default = "default_max_memory_chars")]
    pub max_memory_chars: i64,

    // ========================================================================
    // Model Selection
    // ========================================================================
    /// Model role for LLM-based memory selection.
    ///
    /// Specifies which configured model role to use for semantic memory
    /// file ranking. Supports any role: "fast", "main", "plan", etc.
    /// Defaults to "fast" for lower latency and cost.
    #[serde(default = "default_memory_selection_model_role")]
    pub memory_selection_model_role: String,
}

fn default_max_lines() -> i32 {
    DEFAULT_MAX_MEMORY_LINES
}

fn default_max_files() -> i32 {
    DEFAULT_MAX_RELEVANT_FILES
}

fn default_max_lines_per_file() -> i32 {
    DEFAULT_MAX_LINES_PER_FILE
}

fn default_search_timeout() -> i64 {
    DEFAULT_RELEVANT_SEARCH_TIMEOUT_MS
}

fn default_max_frontmatter_lines() -> i32 {
    DEFAULT_MAX_FRONTMATTER_LINES
}

fn default_staleness_warning_days() -> i32 {
    DEFAULT_STALENESS_WARNING_DAYS
}

fn default_relevant_memories_throttle_turns() -> i32 {
    DEFAULT_RELEVANT_MEMORIES_THROTTLE_TURNS
}

fn default_max_files_to_scan() -> i32 {
    DEFAULT_MAX_FILES_TO_SCAN
}

fn default_min_keyword_length() -> i32 {
    DEFAULT_MIN_KEYWORD_LENGTH
}

fn default_max_memory_chars() -> i64 {
    DEFAULT_MAX_MEMORY_CHARS
}

fn default_memory_selection_model_role() -> String {
    DEFAULT_MEMORY_SELECTION_MODEL_ROLE.to_string()
}

impl Default for AutoMemoryConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            directory: None,
            max_lines: DEFAULT_MAX_MEMORY_LINES,
            max_relevant_files: DEFAULT_MAX_RELEVANT_FILES,
            max_lines_per_file: DEFAULT_MAX_LINES_PER_FILE,
            relevant_search_timeout_ms: DEFAULT_RELEVANT_SEARCH_TIMEOUT_MS,
            max_frontmatter_lines: DEFAULT_MAX_FRONTMATTER_LINES,
            staleness_warning_days: DEFAULT_STALENESS_WARNING_DAYS,
            relevant_memories_throttle_turns: DEFAULT_RELEVANT_MEMORIES_THROTTLE_TURNS,
            max_files_to_scan: DEFAULT_MAX_FILES_TO_SCAN,
            min_keyword_length: DEFAULT_MIN_KEYWORD_LENGTH,
            max_memory_chars: DEFAULT_MAX_MEMORY_CHARS,
            memory_selection_model_role: DEFAULT_MEMORY_SELECTION_MODEL_ROLE.to_string(),
        }
    }
}

#[cfg(test)]
#[path = "auto_memory_config.test.rs"]
mod tests;
