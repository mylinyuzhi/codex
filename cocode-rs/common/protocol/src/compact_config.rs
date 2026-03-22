//! Compaction and session memory configuration.
//!
//! Defines settings for automatic context compaction and session memory management.

use serde::Deserialize;
use serde::Serialize;

// ============================================================================
// Session Memory Constants
// ============================================================================

/// Default minimum tokens for session memory extraction.
pub const DEFAULT_SESSION_MEMORY_MIN_TOKENS: i32 = 10000;

/// Default maximum tokens for session memory extraction.
pub const DEFAULT_SESSION_MEMORY_MAX_TOKENS: i32 = 40000;

/// Default cooldown in seconds between memory extractions.
pub const DEFAULT_EXTRACTION_COOLDOWN_SECS: i32 = 60;

// ============================================================================
// Context Restoration Constants
// ============================================================================

/// Default maximum files for context restoration.
pub const DEFAULT_CONTEXT_RESTORE_MAX_FILES: i32 = 5;

/// Default token budget for context restoration.
pub const DEFAULT_CONTEXT_RESTORE_BUDGET: i32 = 50000;

/// Default maximum tokens per file during context restoration.
pub const DEFAULT_MAX_TOKENS_PER_FILE: i32 = 5000;

// ============================================================================
// Threshold Control Constants
// ============================================================================

/// Auto-compact target buffer (minimum tokens to preserve).
pub const DEFAULT_MIN_TOKENS_TO_PRESERVE: i32 = 13000;

/// Warning threshold offset.
pub const DEFAULT_WARNING_THRESHOLD_OFFSET: i32 = 20000;

/// Error threshold offset.
pub const DEFAULT_ERROR_THRESHOLD_OFFSET: i32 = 20000;

/// Hard blocking limit offset.
///
/// Claude Code uses `context_limit - 13000` as the blocking threshold.
pub const DEFAULT_MIN_BLOCKING_OFFSET: i32 = 13000;

// ============================================================================
// Micro-Compact Constants
// ============================================================================

/// Micro-compact minimum savings tokens.
pub const DEFAULT_MICRO_COMPACT_MIN_SAVINGS: i32 = 20000;

/// Micro-compact trigger threshold.
pub const DEFAULT_MICRO_COMPACT_THRESHOLD: i32 = 40000;

/// Number of recent tool results to keep during micro-compaction.
pub const DEFAULT_RECENT_TOOL_RESULTS_TO_KEEP: i32 = 3;

/// Number of recent turns to preserve files from during micro-compaction.
pub const DEFAULT_MICRO_COMPACT_KEEP_RECENT_TURNS: i32 = 3;

// ============================================================================
// Full Compact Constants
// ============================================================================

/// Maximum retries for summary generation.
pub const DEFAULT_MAX_SUMMARY_RETRIES: i32 = 2;

/// Maximum output tokens for compact operation.
pub const DEFAULT_MAX_COMPACT_OUTPUT_TOKENS: i32 = 16000;

/// Token estimation safety margin coefficient (1.33x).
pub const DEFAULT_TOKEN_SAFETY_MARGIN: f64 = 1.3333333333333333;

/// Token estimate for images.
pub const DEFAULT_TOKENS_PER_IMAGE: i32 = 1334;

// ============================================================================
// Canonical Token Estimation
// ============================================================================

/// Estimate token count from text length using a single canonical formula.
///
/// Uses `ceil(len / 3.0)` which matches Claude Code's `Math.ceil(len * 1.3333)`
/// at ~0.33 tokens per character. All token estimation in the compaction system
/// should use this function to avoid inconsistencies.
pub fn estimate_text_tokens(text: &str) -> i32 {
    (text.len() as f64 / 3.0).ceil() as i32
}

// ============================================================================
// Session Memory Extraction Constants (for background extraction agent)
// ============================================================================

/// Minimum tokens accumulated before first extraction (default: 5,000).
pub const DEFAULT_EXTRACTION_MIN_TOKENS_TO_INIT: i32 = 5000;

/// Minimum tokens between extractions (default: 5,000).
pub const DEFAULT_EXTRACTION_MIN_TOKENS_BETWEEN: i32 = 5000;

/// Minimum tool calls between extractions (default: 10).
pub const DEFAULT_EXTRACTION_TOOL_CALLS_BETWEEN: i32 = 10;

/// Maximum tokens for the summary (default: 4,000).
pub const DEFAULT_EXTRACTION_MAX_SUMMARY_TOKENS: i32 = 4000;

// ============================================================================
// Keep Window Constants (for token-based message retention)
// ============================================================================

/// Minimum tokens to keep in the "keep window" after compaction.
pub const DEFAULT_KEEP_WINDOW_MIN_TOKENS: i32 = 10000;

/// Minimum text messages to keep (ensures recent context is preserved).
pub const DEFAULT_KEEP_WINDOW_MIN_TEXT_MESSAGES: i32 = 5;

/// Maximum tokens in the keep window (prevents keeping too much).
pub const DEFAULT_KEEP_WINDOW_MAX_TOKENS: i32 = 40000;

// ============================================================================
// File Restoration Exclusion Patterns
// ============================================================================

/// Default patterns to exclude from file restoration after compaction.
pub const DEFAULT_EXCLUDED_PATTERNS: &[&str] = &[
    // Agent transcript files
    "*.jsonl",
    "**/transcript*.json",
    // Plan mode files
    "**/plan.md",
    "**/plan-*.md",
    // Instruction files
    "**/CLAUDE.md",
    "**/AGENTS.md",
    // Session memory files
    "**/summary.md",
    "**/session-memory/*",
];

// ============================================================================
// CompactConfig
// ============================================================================

/// Compaction and session memory configuration.
///
/// Controls automatic context compaction behavior and session memory management.
///
/// # Environment Variables
///
/// - `DISABLE_COMPACT`: Completely disable compaction feature
/// - `DISABLE_AUTO_COMPACT`: Disable automatic compaction (manual still works)
/// - `DISABLE_MICRO_COMPACT`: Disable micro-compaction (frequent small compactions)
/// - `COCODE_AUTOCOMPACT_PCT_OVERRIDE`: Override effective context window percentage (0-100)
/// - `COCODE_BLOCKING_LIMIT_OVERRIDE`: Override blocking limit for compaction
/// - `COCODE_SESSION_MEMORY_MIN_TOKENS`: Minimum tokens for session memory
/// - `COCODE_SESSION_MEMORY_MAX_TOKENS`: Maximum tokens for session memory
/// - `COCODE_EXTRACTION_COOLDOWN_SECS`: Cooldown between memory extractions
/// - `COCODE_CONTEXT_RESTORE_MAX_FILES`: Maximum files for context restoration
/// - `COCODE_CONTEXT_RESTORE_BUDGET`: Token budget for context restoration
/// - `COCODE_MIN_TOKENS_TO_PRESERVE`: Auto-compact target buffer
/// - `COCODE_WARNING_THRESHOLD_OFFSET`: Warning threshold offset
/// - `COCODE_ERROR_THRESHOLD_OFFSET`: Error threshold offset
/// - `COCODE_MIN_BLOCKING_OFFSET`: Hard blocking limit offset
/// - `COCODE_MICRO_COMPACT_MIN_SAVINGS`: Micro-compact minimum savings tokens
/// - `COCODE_MICRO_COMPACT_THRESHOLD`: Micro-compact trigger threshold
/// - `COCODE_RECENT_TOOL_RESULTS_TO_KEEP`: Recent tool results to keep
/// - `COCODE_MAX_SUMMARY_RETRIES`: Maximum retries for summary generation
/// - `COCODE_MAX_COMPACT_OUTPUT_TOKENS`: Maximum output tokens for compact
/// - `COCODE_TOKEN_SAFETY_MARGIN`: Token estimation safety margin
/// - `COCODE_TOKENS_PER_IMAGE`: Token estimate for images
/// - `COCODE_MAX_TOKENS_PER_FILE`: Maximum tokens per file
///
/// # Example
///
/// ```json
/// {
///   "compact": {
///     "disable_compact": false,
///     "disable_auto_compact": false,
///     "session_memory_min_tokens": 15000,
///     "session_memory_max_tokens": 50000,
///     "min_tokens_to_preserve": 13000,
///     "micro_compact_min_savings": 20000
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CompactConfig {
    // ========================================================================
    // Feature Toggles
    // ========================================================================
    /// Completely disable compaction feature.
    #[serde(default)]
    pub disable_compact: bool,

    /// Disable automatic compaction (manual still works).
    #[serde(default)]
    pub disable_auto_compact: bool,

    /// Disable micro-compaction (frequent small compactions).
    #[serde(default)]
    pub disable_micro_compact: bool,

    // ========================================================================
    // Override Values
    // ========================================================================
    /// Override auto-compact percentage threshold (0-100).
    ///
    /// Can also be set per-model via `ModelInfo.auto_compact_pct`.
    #[serde(default)]
    pub auto_compact_pct: Option<i32>,

    /// Override blocking limit for compaction.
    #[serde(default)]
    pub blocking_limit_override: Option<i32>,

    // ========================================================================
    // Session Memory
    // ========================================================================
    /// Minimum tokens for session memory extraction.
    #[serde(default = "default_session_memory_min_tokens")]
    pub session_memory_min_tokens: i32,

    /// Maximum tokens for session memory extraction.
    #[serde(default = "default_session_memory_max_tokens")]
    pub session_memory_max_tokens: i32,

    /// Cooldown in seconds between memory extractions.
    #[serde(default = "default_extraction_cooldown_secs")]
    pub extraction_cooldown_secs: i32,

    // ========================================================================
    // Context Restoration
    // ========================================================================
    /// Maximum files for context restoration.
    #[serde(default = "default_context_restore_max_files")]
    pub context_restore_max_files: i32,

    /// Token budget for context restoration.
    #[serde(default = "default_context_restore_budget")]
    pub context_restore_budget: i32,

    /// Maximum tokens per file during context restoration.
    #[serde(default = "default_max_tokens_per_file")]
    pub max_tokens_per_file: i32,

    // ========================================================================
    // Threshold Control
    // ========================================================================
    /// Auto-compact target buffer (minimum tokens to preserve).
    #[serde(default = "default_min_tokens_to_preserve")]
    pub min_tokens_to_preserve: i32,

    /// Warning threshold offset.
    #[serde(default = "default_warning_threshold_offset")]
    pub warning_threshold_offset: i32,

    /// Error threshold offset.
    #[serde(default = "default_error_threshold_offset")]
    pub error_threshold_offset: i32,

    /// Hard blocking limit offset.
    #[serde(default = "default_min_blocking_offset")]
    pub min_blocking_offset: i32,

    // ========================================================================
    // Micro-Compact
    // ========================================================================
    /// Micro-compact minimum savings tokens.
    #[serde(default = "default_micro_compact_min_savings")]
    pub micro_compact_min_savings: i32,

    /// Micro-compact trigger threshold.
    #[serde(default = "default_micro_compact_threshold")]
    pub micro_compact_threshold: i32,

    /// Number of recent tool results to keep during micro-compaction.
    #[serde(default = "default_recent_tool_results_to_keep")]
    pub recent_tool_results_to_keep: i32,

    /// Number of recent turns to preserve files from during micro-compaction.
    #[serde(default = "default_micro_compact_keep_recent_turns")]
    pub micro_compact_keep_recent_turns: i32,

    // ========================================================================
    // Full Compact
    // ========================================================================
    /// Maximum retries for summary generation.
    #[serde(default = "default_max_summary_retries")]
    pub max_summary_retries: i32,

    /// Maximum output tokens for compact operation.
    #[serde(default = "default_max_compact_output_tokens")]
    pub max_compact_output_tokens: i32,

    /// Token estimation safety margin coefficient (1.33x).
    #[serde(default = "default_token_safety_margin")]
    pub token_safety_margin: f64,

    /// Token estimate for images.
    #[serde(default = "default_tokens_per_image")]
    pub tokens_per_image: i32,

    // ========================================================================
    // Keep Window Configuration
    // ========================================================================
    /// Keep window configuration for token-based message retention.
    #[serde(default)]
    pub keep_window: KeepWindowConfig,

    // ========================================================================
    // File Restoration Configuration
    // ========================================================================
    /// File restoration configuration with exclusion patterns.
    #[serde(default)]
    pub file_restoration: FileRestorationConfig,

    // ========================================================================
    // Session Memory Compact (Tier 1) Configuration
    // ========================================================================
    /// Whether Tier 1 session memory compact is enabled.
    ///
    /// When true and a cached summary.md exists, compaction uses it for
    /// zero-API-cost context reduction. When false, always falls back to
    /// full LLM compact. Can be overridden via `COCODE_ENABLE_SM_COMPACT`.
    #[serde(default = "crate::default_true")]
    pub enable_sm_compact: bool,

    /// Path to the session memory file (summary.md) for Tier 1 compaction.
    #[serde(default)]
    pub summary_path: Option<std::path::PathBuf>,

    // ========================================================================
    // Session Memory Extraction Configuration
    // ========================================================================
    /// Background session memory extraction agent configuration.
    #[serde(default)]
    pub session_memory_extraction: SessionMemoryExtractionConfig,
}

// ============================================================================
// KeepWindowConfig
// ============================================================================

/// Configuration for token-based message retention during compaction.
///
/// The "keep window" determines which recent messages are preserved verbatim
/// after compaction, based on token counts rather than simple message counts.
///
/// This mirrors Claude Code's `calculateKeepStartIndex()` logic which ensures:
/// 1. At least `min_tokens` worth of recent messages are kept
/// 2. At least `min_text_messages` text messages are kept
/// 3. No more than `max_tokens` worth of messages are kept
/// 4. Tool use/result pairs are kept together
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct KeepWindowConfig {
    /// Minimum tokens to keep in the window (default: 10,000).
    #[serde(default = "default_keep_window_min_tokens")]
    pub min_tokens: i32,

    /// Minimum text messages to keep (default: 5).
    #[serde(default = "default_keep_window_min_text_messages")]
    pub min_text_messages: i32,

    /// Maximum tokens in the keep window (default: 40,000).
    #[serde(default = "default_keep_window_max_tokens")]
    pub max_tokens: i32,
}

impl Default for KeepWindowConfig {
    fn default() -> Self {
        Self {
            min_tokens: DEFAULT_KEEP_WINDOW_MIN_TOKENS,
            min_text_messages: DEFAULT_KEEP_WINDOW_MIN_TEXT_MESSAGES,
            max_tokens: DEFAULT_KEEP_WINDOW_MAX_TOKENS,
        }
    }
}

/// Validate that a field meets a minimum value, returning a formatted error if not.
macro_rules! validate_min {
    ($self:ident . $field:ident >= $min:expr) => {
        if $self.$field < $min {
            return Err(format!(
                concat!(stringify!($field), " must be >= {}, got {}"),
                $min, $self.$field
            ));
        }
    };
    ($self:ident . $field:ident >= $min:expr, prefix: $prefix:literal) => {
        if $self.$field < $min {
            return Err(format!(
                concat!($prefix, ".", stringify!($field), " must be >= {}, got {}"),
                $min, $self.$field
            ));
        }
    };
}

impl KeepWindowConfig {
    /// Validate configuration values.
    pub fn validate(&self) -> Result<(), String> {
        validate_min!(self.min_tokens >= 0, prefix: "keep_window");
        validate_min!(self.min_text_messages >= 0, prefix: "keep_window");
        if self.max_tokens < self.min_tokens {
            return Err(format!(
                "keep_window.max_tokens ({}) must be >= min_tokens ({})",
                self.max_tokens, self.min_tokens
            ));
        }
        Ok(())
    }
}

// ============================================================================
// SessionMemoryExtractionConfig
// ============================================================================

/// Configuration for background session memory extraction agent.
///
/// The extraction agent runs asynchronously during normal conversation to
/// proactively update the session memory (summary.md). This enables "zero API
/// cost" compaction at critical moments because a cached summary is available.
///
/// # Trigger Conditions
///
/// Extraction is triggered when ALL of the following are true:
/// - Not currently compacting
/// - Either:
///   - First extraction: `min_tokens_to_init` tokens accumulated
///   - Subsequent: `min_tokens_between` tokens AND `tool_calls_between` tool calls
///     since last extraction, AND cooldown elapsed
///
/// # Example
///
/// ```json
/// {
///   "session_memory_extraction": {
///     "enabled": true,
///     "min_tokens_to_init": 5000,
///     "min_tokens_between": 5000,
///     "tool_calls_between": 10,
///     "cooldown_secs": 60,
///     "max_summary_tokens": 4000
///   }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionMemoryExtractionConfig {
    /// Enable background extraction agent.
    #[serde(default = "crate::default_true")]
    pub enabled: bool,

    /// Minimum tokens accumulated before first extraction (default: 5,000).
    #[serde(default = "default_extraction_min_tokens_to_init")]
    pub min_tokens_to_init: i32,

    /// Minimum tokens between extractions (default: 5,000).
    #[serde(default = "default_extraction_min_tokens_between")]
    pub min_tokens_between: i32,

    /// Minimum tool calls between extractions (default: 10).
    #[serde(default = "default_extraction_tool_calls_between")]
    pub tool_calls_between: i32,

    /// Cooldown between extractions in seconds (default: 60).
    #[serde(default = "default_extraction_cooldown_secs")]
    pub cooldown_secs: i32,

    /// Maximum tokens for the summary (default: 4,000).
    #[serde(default = "default_extraction_max_summary_tokens")]
    pub max_summary_tokens: i32,
}

impl Default for SessionMemoryExtractionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_tokens_to_init: DEFAULT_EXTRACTION_MIN_TOKENS_TO_INIT,
            min_tokens_between: DEFAULT_EXTRACTION_MIN_TOKENS_BETWEEN,
            tool_calls_between: DEFAULT_EXTRACTION_TOOL_CALLS_BETWEEN,
            cooldown_secs: DEFAULT_EXTRACTION_COOLDOWN_SECS,
            max_summary_tokens: DEFAULT_EXTRACTION_MAX_SUMMARY_TOKENS,
        }
    }
}

impl SessionMemoryExtractionConfig {
    /// Load configuration with environment variable overrides.
    ///
    /// Supported environment variables:
    /// - `COCODE_ENABLE_SESSION_MEMORY`: Enable/disable background extraction (true/false)
    /// - `COCODE_EXTRACTION_MIN_TOKENS`: Override min_tokens_to_init and min_tokens_between
    /// - `COCODE_EXTRACTION_TOOL_CALLS`: Override tool_calls_between
    /// - `COCODE_EXTRACTION_COOLDOWN`: Override cooldown_secs
    /// - `COCODE_EXTRACTION_MAX_TOKENS`: Override max_summary_tokens
    pub fn with_env_overrides(mut self) -> Self {
        // COCODE_ENABLE_SESSION_MEMORY - master toggle
        if let Ok(val) = std::env::var("COCODE_ENABLE_SESSION_MEMORY")
            && let Ok(enabled) = val.parse::<bool>()
        {
            self.enabled = enabled;
        }

        // COCODE_EXTRACTION_MIN_TOKENS - trigger threshold
        if let Ok(val) = std::env::var("COCODE_EXTRACTION_MIN_TOKENS")
            && let Ok(tokens) = val.parse::<i32>()
        {
            self.min_tokens_to_init = tokens;
            self.min_tokens_between = tokens;
        }

        // COCODE_EXTRACTION_TOOL_CALLS - tool call threshold
        if let Ok(val) = std::env::var("COCODE_EXTRACTION_TOOL_CALLS")
            && let Ok(calls) = val.parse::<i32>()
        {
            self.tool_calls_between = calls;
        }

        // COCODE_EXTRACTION_COOLDOWN - cooldown seconds
        if let Ok(val) = std::env::var("COCODE_EXTRACTION_COOLDOWN")
            && let Ok(secs) = val.parse::<i32>()
        {
            self.cooldown_secs = secs;
        }

        // COCODE_EXTRACTION_MAX_TOKENS - max summary tokens
        if let Ok(val) = std::env::var("COCODE_EXTRACTION_MAX_TOKENS")
            && let Ok(tokens) = val.parse::<i32>()
        {
            self.max_summary_tokens = tokens;
        }

        self
    }

    /// Validate configuration values.
    pub fn validate(&self) -> Result<(), String> {
        validate_min!(self.min_tokens_to_init >= 0, prefix: "session_memory_extraction");
        validate_min!(self.min_tokens_between >= 0, prefix: "session_memory_extraction");
        validate_min!(self.tool_calls_between >= 0, prefix: "session_memory_extraction");
        validate_min!(self.cooldown_secs >= 0, prefix: "session_memory_extraction");
        validate_min!(self.max_summary_tokens >= 0, prefix: "session_memory_extraction");
        Ok(())
    }
}

fn default_extraction_min_tokens_to_init() -> i32 {
    DEFAULT_EXTRACTION_MIN_TOKENS_TO_INIT
}

fn default_extraction_min_tokens_between() -> i32 {
    DEFAULT_EXTRACTION_MIN_TOKENS_BETWEEN
}

fn default_extraction_tool_calls_between() -> i32 {
    DEFAULT_EXTRACTION_TOOL_CALLS_BETWEEN
}

fn default_extraction_max_summary_tokens() -> i32 {
    DEFAULT_EXTRACTION_MAX_SUMMARY_TOKENS
}

// ============================================================================
// FileRestorationConfig
// ============================================================================

/// Configuration for file restoration after compaction.
///
/// Controls which files are restored and how much content is included
/// in the restored context after compaction.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FileRestorationConfig {
    /// Maximum number of files to restore (default: 5).
    #[serde(default = "default_context_restore_max_files")]
    pub max_files: i32,

    /// Maximum tokens per file (default: 5,000).
    #[serde(default = "default_max_tokens_per_file")]
    pub max_tokens_per_file: i32,

    /// Total token budget for restoration (default: 50,000).
    #[serde(default = "default_context_restore_budget")]
    pub total_token_budget: i32,

    /// Glob patterns to exclude from restoration.
    #[serde(default = "default_excluded_patterns_vec")]
    pub excluded_patterns: Vec<String>,

    /// Whether to sort files by last access time (most recent first).
    #[serde(default = "default_sort_by_access")]
    pub sort_by_access_time: bool,
}

impl Default for FileRestorationConfig {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_CONTEXT_RESTORE_MAX_FILES,
            max_tokens_per_file: DEFAULT_MAX_TOKENS_PER_FILE,
            total_token_budget: DEFAULT_CONTEXT_RESTORE_BUDGET,
            excluded_patterns: default_excluded_patterns_vec(),
            sort_by_access_time: true,
        }
    }
}

impl FileRestorationConfig {
    /// Check if a file path should be excluded from restoration.
    pub fn should_exclude(&self, path: &str) -> bool {
        for pattern in &self.excluded_patterns {
            if Self::matches_glob(pattern, path) {
                return true;
            }
        }
        false
    }

    /// Simple glob matching (supports * and ** patterns).
    fn matches_glob(pattern: &str, path: &str) -> bool {
        // Handle ** (match any path segment)
        if pattern.contains("**") {
            let parts: Vec<&str> = pattern.split("**").collect();
            if parts.len() == 2 {
                let prefix = parts[0].trim_end_matches('/');
                let suffix = parts[1].trim_start_matches('/');

                // Check prefix
                if !prefix.is_empty() && !path.starts_with(prefix) {
                    return false;
                }

                // Handle suffix patterns with wildcards (like "plan-*.md")
                if !suffix.is_empty() {
                    return Self::matches_suffix_pattern(suffix, path);
                }

                // Empty suffix means match anything
                return true;
            }
        }

        // Handle simple * wildcard
        if pattern.starts_with('*') && !pattern.contains('/') {
            // *.ext pattern - match file extension
            let ext = &pattern[1..];
            return path.ends_with(ext);
        }

        // Handle pattern with * in the middle (like "plan-*.md")
        if pattern.contains('*') && !pattern.contains('/') {
            return Self::matches_suffix_pattern(pattern, path);
        }

        // Exact match
        path == pattern || path.ends_with(&format!("/{pattern}"))
    }

    /// Match a suffix pattern that may contain wildcards against a path.
    fn matches_suffix_pattern(suffix_pattern: &str, path: &str) -> bool {
        // Get the filename from the path
        let filename = path.rsplit('/').next().unwrap_or(path);

        // Handle patterns like "plan-*.md"
        if let Some(star_pos) = suffix_pattern.find('*') {
            let prefix = &suffix_pattern[..star_pos];
            let suffix = &suffix_pattern[star_pos + 1..];

            // Check if filename matches prefix*suffix pattern
            if filename.starts_with(prefix) && filename.ends_with(suffix) {
                // Make sure prefix + suffix don't overlap
                return filename.len() >= prefix.len() + suffix.len();
            }
            return false;
        }

        // No wildcard - exact filename match
        filename == suffix_pattern || path.ends_with(&format!("/{suffix_pattern}"))
    }

    /// Validate configuration values.
    pub fn validate(&self) -> Result<(), String> {
        validate_min!(self.max_files >= 0, prefix: "file_restoration");
        validate_min!(self.max_tokens_per_file >= 0, prefix: "file_restoration");
        validate_min!(self.total_token_budget >= 0, prefix: "file_restoration");
        Ok(())
    }
}

fn default_keep_window_min_tokens() -> i32 {
    DEFAULT_KEEP_WINDOW_MIN_TOKENS
}

fn default_keep_window_min_text_messages() -> i32 {
    DEFAULT_KEEP_WINDOW_MIN_TEXT_MESSAGES
}

fn default_keep_window_max_tokens() -> i32 {
    DEFAULT_KEEP_WINDOW_MAX_TOKENS
}

fn default_excluded_patterns_vec() -> Vec<String> {
    DEFAULT_EXCLUDED_PATTERNS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

fn default_sort_by_access() -> bool {
    true
}

impl Default for CompactConfig {
    fn default() -> Self {
        Self {
            // Feature toggles
            disable_compact: false,
            disable_auto_compact: false,
            disable_micro_compact: false,
            // Overrides — Claude Code triggers auto-compact at 80% context usage
            auto_compact_pct: Some(80),
            blocking_limit_override: None,
            // Session memory
            session_memory_min_tokens: DEFAULT_SESSION_MEMORY_MIN_TOKENS,
            session_memory_max_tokens: DEFAULT_SESSION_MEMORY_MAX_TOKENS,
            extraction_cooldown_secs: DEFAULT_EXTRACTION_COOLDOWN_SECS,
            // Context restoration
            context_restore_max_files: DEFAULT_CONTEXT_RESTORE_MAX_FILES,
            context_restore_budget: DEFAULT_CONTEXT_RESTORE_BUDGET,
            max_tokens_per_file: DEFAULT_MAX_TOKENS_PER_FILE,
            // Threshold control
            min_tokens_to_preserve: DEFAULT_MIN_TOKENS_TO_PRESERVE,
            warning_threshold_offset: DEFAULT_WARNING_THRESHOLD_OFFSET,
            error_threshold_offset: DEFAULT_ERROR_THRESHOLD_OFFSET,
            min_blocking_offset: DEFAULT_MIN_BLOCKING_OFFSET,
            // Micro-compact
            micro_compact_min_savings: DEFAULT_MICRO_COMPACT_MIN_SAVINGS,
            micro_compact_threshold: DEFAULT_MICRO_COMPACT_THRESHOLD,
            recent_tool_results_to_keep: DEFAULT_RECENT_TOOL_RESULTS_TO_KEEP,
            micro_compact_keep_recent_turns: DEFAULT_MICRO_COMPACT_KEEP_RECENT_TURNS,
            // Full compact
            max_summary_retries: DEFAULT_MAX_SUMMARY_RETRIES,
            max_compact_output_tokens: DEFAULT_MAX_COMPACT_OUTPUT_TOKENS,
            token_safety_margin: DEFAULT_TOKEN_SAFETY_MARGIN,
            tokens_per_image: DEFAULT_TOKENS_PER_IMAGE,
            // Keep window
            keep_window: KeepWindowConfig::default(),
            // File restoration
            file_restoration: FileRestorationConfig::default(),
            // Session memory compact (Tier 1)
            enable_sm_compact: true,
            summary_path: None,
            // Session memory extraction
            session_memory_extraction: SessionMemoryExtractionConfig::default(),
        }
    }
}

impl CompactConfig {
    /// Check if compaction is enabled (not disabled).
    pub fn is_compaction_enabled(&self) -> bool {
        !self.disable_compact
    }

    /// Check if auto-compaction is enabled.
    pub fn is_auto_compact_enabled(&self) -> bool {
        !self.disable_compact && !self.disable_auto_compact
    }

    /// Check if micro-compaction is enabled.
    pub fn is_micro_compact_enabled(&self) -> bool {
        !self.disable_compact && !self.disable_micro_compact
    }

    /// Calculate auto-compact target based on available tokens.
    ///
    /// If `auto_compact_pct` is set, uses that percentage of available tokens,
    /// capped at `available_tokens - min_tokens_to_preserve`.
    pub fn auto_compact_target(&self, available_tokens: i32) -> i32 {
        if let Some(pct) = self.auto_compact_pct {
            let calculated = (available_tokens as f64 * (pct as f64 / 100.0)).floor() as i32;
            calculated.min(available_tokens - self.min_tokens_to_preserve)
        } else {
            available_tokens - self.min_tokens_to_preserve
        }
    }

    /// Calculate blocking limit based on available tokens.
    ///
    /// Uses `blocking_limit_override` if set, otherwise `available_tokens - min_blocking_offset`.
    pub fn blocking_limit(&self, available_tokens: i32) -> i32 {
        self.blocking_limit_override
            .unwrap_or(available_tokens - self.min_blocking_offset)
    }

    /// Calculate warning threshold based on target.
    pub fn warning_threshold(&self, target: i32) -> i32 {
        target - self.warning_threshold_offset
    }

    /// Calculate error threshold based on target.
    pub fn error_threshold(&self, target: i32) -> i32 {
        target - self.error_threshold_offset
    }

    /// Apply safety margin to token estimate.
    pub fn estimate_tokens_with_margin(&self, base_estimate: i32) -> i32 {
        (base_estimate as f64 * self.token_safety_margin).ceil() as i32
    }

    /// Validate configuration values.
    ///
    /// Returns an error message if any values are invalid.
    pub fn validate(&self) -> Result<(), String> {
        // Validate auto_compact_pct
        if let Some(pct) = self.auto_compact_pct
            && !(0..=100).contains(&pct)
        {
            return Err(format!("auto_compact_pct must be 0-100, got {pct}"));
        }

        // Validate session memory tokens
        if self.session_memory_min_tokens > self.session_memory_max_tokens {
            return Err(format!(
                "session_memory_min_tokens ({}) > session_memory_max_tokens ({})",
                self.session_memory_min_tokens, self.session_memory_max_tokens
            ));
        }

        validate_min!(self.extraction_cooldown_secs >= 0);
        validate_min!(self.context_restore_max_files >= 0);
        validate_min!(self.context_restore_budget >= 0);
        validate_min!(self.max_tokens_per_file >= 0);
        validate_min!(self.min_tokens_to_preserve >= 0);
        validate_min!(self.micro_compact_min_savings >= 0);
        validate_min!(self.recent_tool_results_to_keep >= 0);
        validate_min!(self.max_summary_retries >= 1);
        validate_min!(self.token_safety_margin >= 1.0);

        // Validate nested configs
        self.keep_window.validate()?;
        self.file_restoration.validate()?;
        self.session_memory_extraction.validate()?;

        Ok(())
    }

    /// Check if session memory extraction is enabled.
    pub fn is_session_memory_extraction_enabled(&self) -> bool {
        !self.disable_compact && self.session_memory_extraction.enabled
    }

    /// Apply environment variable overrides to compact config.
    ///
    /// Reads environment variables and overrides the corresponding config values.
    /// This should be called after constructing the config from JSON/defaults.
    ///
    /// Supported variables:
    /// - `COCODE_DISABLE_COMPACT` → disable all compaction (set to "1" or "true")
    /// - `COCODE_DISABLE_AUTO_COMPACT` → disable auto-compaction only
    /// - `COCODE_AUTOCOMPACT_PCT_OVERRIDE` → percentage-based threshold (0-100)
    /// - `COCODE_BLOCKING_LIMIT_OVERRIDE` → custom hard limit
    pub fn with_env_overrides(mut self) -> Self {
        if let Ok(val) = std::env::var("COCODE_DISABLE_COMPACT")
            && (val == "1" || val.eq_ignore_ascii_case("true"))
        {
            self.disable_compact = true;
        }
        if let Ok(val) = std::env::var("COCODE_DISABLE_AUTO_COMPACT")
            && (val == "1" || val.eq_ignore_ascii_case("true"))
        {
            self.disable_auto_compact = true;
        }
        if let Ok(val) = std::env::var("COCODE_AUTOCOMPACT_PCT_OVERRIDE")
            && let Ok(pct) = val.parse::<i32>()
            && (0..=100).contains(&pct)
        {
            self.auto_compact_pct = Some(pct);
        }
        if let Ok(val) = std::env::var("COCODE_BLOCKING_LIMIT_OVERRIDE")
            && let Ok(limit) = val.parse::<i32>()
            && limit > 0
        {
            self.blocking_limit_override = Some(limit);
        }
        if let Ok(val) = std::env::var("COCODE_ENABLE_SM_COMPACT")
            && let Ok(enabled) = val.parse::<bool>()
        {
            self.enable_sm_compact = enabled;
        }
        self
    }

    /// Apply model-level overrides to compact config.
    ///
    /// If the model specifies `auto_compact_pct` and this config
    /// doesn't already have one set (by env or JSON), use the model's value.
    pub fn apply_model_overrides(&mut self, model_info: &crate::ModelInfo) {
        if let Some(pct) = model_info.auto_compact_pct
            && self.auto_compact_pct.is_none()
        {
            self.auto_compact_pct = Some(pct);
        }
    }
}

// ============================================================================
// Default value functions for serde
// ============================================================================

fn default_session_memory_min_tokens() -> i32 {
    DEFAULT_SESSION_MEMORY_MIN_TOKENS
}

fn default_session_memory_max_tokens() -> i32 {
    DEFAULT_SESSION_MEMORY_MAX_TOKENS
}

fn default_extraction_cooldown_secs() -> i32 {
    DEFAULT_EXTRACTION_COOLDOWN_SECS
}

fn default_context_restore_max_files() -> i32 {
    DEFAULT_CONTEXT_RESTORE_MAX_FILES
}

fn default_context_restore_budget() -> i32 {
    DEFAULT_CONTEXT_RESTORE_BUDGET
}

fn default_max_tokens_per_file() -> i32 {
    DEFAULT_MAX_TOKENS_PER_FILE
}

fn default_min_tokens_to_preserve() -> i32 {
    DEFAULT_MIN_TOKENS_TO_PRESERVE
}

fn default_warning_threshold_offset() -> i32 {
    DEFAULT_WARNING_THRESHOLD_OFFSET
}

fn default_error_threshold_offset() -> i32 {
    DEFAULT_ERROR_THRESHOLD_OFFSET
}

fn default_min_blocking_offset() -> i32 {
    DEFAULT_MIN_BLOCKING_OFFSET
}

fn default_micro_compact_min_savings() -> i32 {
    DEFAULT_MICRO_COMPACT_MIN_SAVINGS
}

fn default_micro_compact_threshold() -> i32 {
    DEFAULT_MICRO_COMPACT_THRESHOLD
}

fn default_recent_tool_results_to_keep() -> i32 {
    DEFAULT_RECENT_TOOL_RESULTS_TO_KEEP
}

fn default_micro_compact_keep_recent_turns() -> i32 {
    DEFAULT_MICRO_COMPACT_KEEP_RECENT_TURNS
}

fn default_max_summary_retries() -> i32 {
    DEFAULT_MAX_SUMMARY_RETRIES
}

fn default_max_compact_output_tokens() -> i32 {
    DEFAULT_MAX_COMPACT_OUTPUT_TOKENS
}

fn default_token_safety_margin() -> f64 {
    DEFAULT_TOKEN_SAFETY_MARGIN
}

fn default_tokens_per_image() -> i32 {
    DEFAULT_TOKENS_PER_IMAGE
}

#[cfg(test)]
#[path = "compact_config.test.rs"]
mod tests;
