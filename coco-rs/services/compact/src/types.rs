//! Core compaction types.

use coco_types::AttachmentMessage;
use coco_types::CompactTrigger;
use coco_types::Message;
use serde::Deserialize;
use serde::Serialize;

// ── TS constants (from autoCompact.ts) ──────────────────────────────

/// Buffer below effective context window for auto-compact trigger.
pub const AUTOCOMPACT_BUFFER_TOKENS: i64 = 13_000;

/// Token buffer before warning threshold.
pub const WARNING_THRESHOLD_BUFFER_TOKENS: i64 = 20_000;

/// Token buffer before error threshold.
pub const ERROR_THRESHOLD_BUFFER_TOKENS: i64 = 20_000;

/// Reserve for manual compact blocking limit.
pub const MANUAL_COMPACT_BUFFER_TOKENS: i64 = 3_000;

/// Max tokens reserved for the compact summary output.
pub const MAX_OUTPUT_TOKENS_FOR_SUMMARY: i64 = 20_000;

/// Circuit breaker: stop after this many consecutive auto-compact failures.
pub const MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES: i32 = 3;

/// Max prompt-too-long retries before giving up.
pub const MAX_PTL_RETRIES: i32 = 3;

/// Max streaming retries for the summary LLM call.
pub const MAX_COMPACT_STREAMING_RETRIES: i32 = 2;

// ── Post-compact attachment budgets (from compact.ts) ───────────────

/// Maximum recently-read files to re-inject after compaction.
pub const POST_COMPACT_MAX_FILES_TO_RESTORE: usize = 5;

/// Total token budget for post-compact file attachments.
pub const POST_COMPACT_TOKEN_BUDGET: i64 = 50_000;

/// Per-file token cap for post-compact restore.
pub const POST_COMPACT_MAX_TOKENS_PER_FILE: i64 = 5_000;

/// Per-skill token cap for post-compact restore.
pub const POST_COMPACT_MAX_TOKENS_PER_SKILL: i64 = 5_000;

/// Total token budget for all skill re-injections.
pub const POST_COMPACT_SKILLS_TOKEN_BUDGET: i64 = 25_000;

/// Token estimate for images/documents stripped before compaction.
pub const IMAGE_MAX_TOKEN_SIZE: i64 = 2_000;

// ── Cleared-content markers (must match TS exactly) ─────────────────

/// Placeholder inserted when old tool result content is cleared.
pub const CLEARED_TOOL_RESULT_MESSAGE: &str = "[Old tool result content cleared]";

/// Synthetic user message prepended on prompt-too-long retry.
pub const PTL_RETRY_MARKER: &str = "[earlier conversation truncated for compaction retry]";

// ── Result types ────────────────────────────────────────────────────

/// Result of a full compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactResult {
    /// Compact boundary marker (system message).
    pub boundary_marker: Message,
    /// Summary messages (the LLM-generated or session-memory summary).
    pub summary_messages: Vec<Message>,
    /// Post-compact attachments (file restore, plan, skills, tools, MCP).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentMessage>,
    /// Messages preserved (not compacted).
    pub messages_to_keep: Vec<Message>,
    /// Hook result messages from pre/post-compact hooks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_results: Vec<Message>,
    /// User-facing status message (shown in TUI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_display_message: Option<String>,
    /// Token count before compaction.
    pub pre_compact_tokens: i64,
    /// Token count after compaction (API call total).
    pub post_compact_tokens: i64,
    /// True post-compact token count (message payload estimate).
    #[serde(default)]
    pub true_post_compact_tokens: i64,
    /// Whether this was a re-compaction.
    #[serde(default)]
    pub is_recompaction: bool,
    /// How compaction was triggered.
    #[serde(default = "default_trigger")]
    pub trigger: CompactTrigger,
}

fn default_trigger() -> CompactTrigger {
    CompactTrigger::Auto
}

/// Info about a re-compaction scenario.
#[derive(Debug, Clone)]
pub struct RecompactionInfo {
    pub is_recompaction: bool,
    pub turns_since_previous: i32,
    pub auto_compact_threshold: i64,
}

/// Result of a micro-compaction (lightweight tool result clearing).
#[derive(Debug, Clone, Default)]
pub struct MicrocompactResult {
    /// Number of tool results cleared.
    pub messages_cleared: i32,
    /// Estimated tokens saved.
    pub tokens_saved_estimate: i64,
    /// Whether triggered by time-based config.
    pub was_time_triggered: bool,
}

/// Token warning state for the TUI/query engine.
#[derive(Debug, Clone)]
pub struct TokenWarningState {
    /// Percentage of context window remaining.
    pub percent_left: i32,
    /// Whether above the warning threshold.
    pub is_above_warning_threshold: bool,
    /// Whether above the error threshold.
    pub is_above_error_threshold: bool,
    /// Whether above the auto-compact threshold.
    pub is_above_auto_compact_threshold: bool,
    /// Whether at the blocking limit (no more input accepted).
    pub is_at_blocking_limit: bool,
}

/// Strategy for API-native context editing.
#[derive(Debug, Clone)]
pub enum ContextEditStrategy {
    /// Clear tool use results (keep tool calls).
    ClearToolUses {
        /// Trigger threshold (token count).
        trigger: Option<i64>,
        /// How many recent tool uses to keep.
        keep_recent: Option<i32>,
        /// Which tool inputs to clear.
        clear_inputs: ClearToolInputs,
        /// Tools excluded from clearing.
        exclude_tools: Vec<String>,
    },
    /// Clear thinking/reasoning content.
    ClearThinking {
        /// How many recent thinking blocks to keep.
        keep_recent_turns: i32,
    },
}

/// Which tool inputs to clear during context editing.
#[derive(Debug, Clone)]
pub enum ClearToolInputs {
    All,
    SpecificTools(Vec<String>),
    None,
}

/// Compaction errors.
#[derive(Debug, thiserror::Error)]
pub enum CompactError {
    /// LLM summarization call failed.
    #[error("LLM call failed: {message}")]
    LlmCallFailed { message: String },
    /// Token budget exceeded.
    #[error("token budget exceeded: {actual} > {limit}")]
    TokenBudgetExceeded { actual: i64, limit: i64 },
    /// Cancelled by user.
    #[error("compaction cancelled")]
    Cancelled,
    /// Stream retry exhausted.
    #[error("stream retry exhausted after {attempts} attempts")]
    StreamRetryExhausted { attempts: i32 },
    /// Prompt too long for summarization.
    #[error("prompt too long: {message}")]
    PromptTooLong { message: String },
}
