//! Context compaction logic for managing conversation history size.
//!
//! This module implements a 3-tier compaction strategy:
//! - **Tier 1 (Session Memory)**: Use cached summary.md - zero API cost
//! - **Tier 2 (Full Compact)**: LLM-based summarization when no cache
//! - **Micro-compact**: Pre-API removal of old tool results (no LLM)
//!
//! Configuration for compaction is centralized in `CompactConfig` from the
//! `cocode_protocol` crate. All threshold constants are configurable through
//! that config struct.
//!
//! ## Micro-Compact Algorithm
//!
//! The micro-compact algorithm runs in 7 phases:
//! 1. Collect tool_use IDs and token counts
//! 2. Determine which tool results need compaction (keep recent N)
//! 3. Check thresholds (warning threshold + minimum savings)
//! 4. Memory attachment cleanup
//! 5. Content replacement (persist or clear marker)
//! 6. readFileState cleanup
//! 7. State update and return
//!
//! ## Compactable Tools
//!
//! Only certain tools have results that can be safely compacted:
//! - Read, Bash, Grep, Glob - file/command output
//! - WebSearch, WebFetch - web content
//! - Edit, Write - file operation confirmations

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;
use tracing::debug;
use tracing::info;
use tracing::warn;

// Re-export commonly used types and constants from protocol for convenience
pub use cocode_protocol::CompactBoundaryMetadata;
pub use cocode_protocol::CompactConfig;
pub use cocode_protocol::CompactTelemetry;
pub use cocode_protocol::CompactTrigger;
pub use cocode_protocol::FileRestorationConfig;
pub use cocode_protocol::HookAdditionalContext;
pub use cocode_protocol::KeepWindowConfig;
pub use cocode_protocol::MemoryAttachment;
pub use cocode_protocol::PersistedToolResult;
pub use cocode_protocol::TokenBreakdown;

// Backwards-compatible re-exports with old names
pub use cocode_protocol::DEFAULT_CONTEXT_RESTORE_BUDGET as CONTEXT_RESTORATION_BUDGET;
pub use cocode_protocol::DEFAULT_CONTEXT_RESTORE_MAX_FILES as CONTEXT_RESTORATION_MAX_FILES;
pub use cocode_protocol::DEFAULT_MICRO_COMPACT_MIN_SAVINGS as MIN_MICRO_COMPACT_SAVINGS;
pub use cocode_protocol::DEFAULT_RECENT_TOOL_RESULTS_TO_KEEP as RECENT_TOOL_RESULTS_TO_KEEP;

// ============================================================================
// Compactable Tools
// ============================================================================

/// Tools whose results can be safely micro-compacted.
///
/// These tools produce output that can be replaced with a placeholder or
/// persisted to disk without losing critical conversation context.
pub static COMPACTABLE_TOOLS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "Read",      // File content - can be re-read
        "Bash",      // Command output - typically verbose
        "Grep",      // Search results - can be re-run
        "Glob",      // File listings - can be re-run
        "WebSearch", // Search results - ephemeral
        "WebFetch",  // Web content - can be re-fetched
        "Edit",      // Edit confirmation - minimal info loss
        "Write",     // Write confirmation - minimal info loss
    ])
});

/// Marker text used to replace cleared tool result content.
pub const CLEARED_CONTENT_MARKER: &str = "[Old tool result content cleared]";

/// Maximum characters to keep as a preview when clearing content.
pub const CONTENT_PREVIEW_LENGTH: usize = 2000;

/// Configuration for context compaction behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Context usage ratio (0.0 - 1.0) at which compaction triggers.
    #[serde(default = "default_threshold")]
    pub threshold: f64,

    /// Whether micro-compaction of large tool results is enabled.
    #[serde(default = "default_micro_compact")]
    pub micro_compact: bool,

    /// Minimum number of messages to retain after compaction.
    #[serde(default = "default_min_messages")]
    pub min_messages_to_keep: i32,

    /// Session memory configuration for Tier 1 compaction.
    #[serde(default)]
    pub session_memory: SessionMemoryConfig,
}

fn default_threshold() -> f64 {
    0.8
}

fn default_micro_compact() -> bool {
    true
}

fn default_min_messages() -> i32 {
    4
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            threshold: default_threshold(),
            micro_compact: default_micro_compact(),
            min_messages_to_keep: default_min_messages(),
            session_memory: SessionMemoryConfig::default(),
        }
    }
}

/// Configuration for session memory (Tier 1 compaction).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemoryConfig {
    /// Whether session memory is enabled.
    #[serde(default = "default_session_memory_enabled")]
    pub enabled: bool,

    /// Whether session memory compact (Tier 1) is enabled.
    ///
    /// When false, always falls back to full LLM compact even if
    /// a cached summary.md is available. Can be controlled via
    /// `COCODE_ENABLE_SM_COMPACT` environment variable.
    #[serde(default = "default_true")]
    pub enable_sm_compact: bool,

    /// Path to the session memory file (summary.md).
    #[serde(default)]
    pub summary_path: Option<PathBuf>,

    /// Minimum tokens to save for session memory to be used.
    #[serde(default = "default_session_memory_min_savings")]
    pub min_savings_tokens: i32,

    /// Last summarized message ID (for incremental updates).
    #[serde(default)]
    pub last_summarized_id: Option<String>,
}

fn default_session_memory_enabled() -> bool {
    false
}

fn default_true() -> bool {
    true
}

fn default_session_memory_min_savings() -> i32 {
    10_000
}

impl Default for SessionMemoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_session_memory_enabled(),
            enable_sm_compact: true,
            summary_path: None,
            min_savings_tokens: default_session_memory_min_savings(),
            last_summarized_id: None,
        }
    }
}

impl SessionMemoryConfig {
    /// Load configuration with environment variable overrides.
    ///
    /// Supported environment variables:
    /// - `COCODE_ENABLE_SM_COMPACT`: Enable/disable session memory compact (true/false)
    pub fn with_env_overrides(mut self) -> Self {
        if let Ok(val) = std::env::var("COCODE_ENABLE_SM_COMPACT")
            && let Ok(enabled) = val.parse::<bool>()
        {
            self.enable_sm_compact = enabled;
        }
        self
    }
}

/// Result of a compaction operation, summarising what was removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionResult {
    /// Number of messages removed during compaction.
    pub removed_messages: i32,

    /// Approximate token count of the generated summary.
    pub summary_tokens: i32,

    /// Number of messages that were micro-compacted (tool output trimmed).
    pub micro_compacted: i32,

    /// The tier of compaction used.
    pub tier: CompactionTier,

    /// Tokens saved by this compaction.
    pub tokens_saved: i32,

    /// Trigger type for this compaction.
    #[serde(default)]
    pub trigger: CompactTrigger,

    /// Telemetry data for this compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<CompactTelemetry>,

    /// Compact boundary metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_metadata: Option<CompactBoundaryMetadata>,

    /// Post-compact hook output contexts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_contexts: Vec<HookAdditionalContext>,

    /// Transcript path for full conversation history reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<PathBuf>,
}

/// Which compaction tier was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionTier {
    /// Tier 1: Session memory (cached summary.md).
    SessionMemory,
    /// Tier 2: Full LLM-based compaction.
    Full,
    /// Micro-compaction only (no summarization).
    Micro,
}

/// Items to restore after compaction.
#[derive(Debug, Clone, Default)]
pub struct ContextRestoration {
    /// Files to restore (path, content, priority).
    pub files: Vec<FileRestoration>,
    /// Todo list state.
    pub todos: Option<String>,
    /// Plan mode state.
    pub plan: Option<String>,
    /// Active skills.
    pub skills: Vec<String>,
    /// Recently invoked skills to restore after compaction.
    pub invoked_skills: Vec<InvokedSkillRestoration>,
    /// Background task status attachments.
    pub task_status: Option<TaskStatusRestoration>,
    /// Memory attachments that were preserved.
    pub memory_attachments: Vec<MemoryAttachment>,
}

/// A recently invoked skill for restoration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokedSkillRestoration {
    /// Skill name.
    pub name: String,
    /// When the skill was last invoked.
    pub last_invoked_turn: i32,
    /// Skill arguments (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
}

/// A file to restore after compaction.
#[derive(Debug, Clone)]
pub struct FileRestoration {
    /// Path to the file.
    pub path: PathBuf,
    /// File content (or summary if too large).
    pub content: String,
    /// Priority for restoration (higher = more important).
    pub priority: i32,
    /// Estimated token count.
    pub tokens: i32,
    /// Last access timestamp (Unix milliseconds) for access-time sorting.
    pub last_accessed: i64,
}

/// Task status for restoration after compaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskStatusRestoration {
    /// Task list in serialized form.
    pub tasks: Vec<TaskInfo>,
}

/// Information about a task for restoration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    /// Task ID.
    pub id: String,
    /// Task subject/title.
    pub subject: String,
    /// Task status (pending, in_progress, completed).
    pub status: String,
    /// Task owner (if assigned).
    pub owner: Option<String>,
}

impl TaskStatusRestoration {
    /// Extract task status from message history tool calls.
    ///
    /// Scans the conversation for TodoWrite tool calls and extracts
    /// the most recent task list for restoration after compaction.
    pub fn from_tool_calls(tool_calls: &[(String, serde_json::Value)]) -> Self {
        // Find the most recent TodoWrite call (scan from end)
        for (name, input) in tool_calls.iter().rev() {
            if name == "TodoWrite"
                && let Some(todos) = input.get("todos").and_then(|t| t.as_array())
            {
                let tasks: Vec<TaskInfo> = todos
                    .iter()
                    .enumerate()
                    .filter_map(|(i, todo)| {
                        let id = todo
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                            .unwrap_or_else(|| format!("{}", i + 1));

                        let subject = todo
                            .get("subject")
                            .or_else(|| todo.get("content"))
                            .and_then(|v| v.as_str())
                            .map(String::from)?;

                        let status = todo
                            .get("status")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                            .unwrap_or_else(|| "pending".to_string());

                        let owner = todo.get("owner").and_then(|v| v.as_str()).map(String::from);

                        Some(TaskInfo {
                            id,
                            subject,
                            status,
                            owner,
                        })
                    })
                    .collect();

                if !tasks.is_empty() {
                    return Self { tasks };
                }
            }
        }

        Self::default()
    }
}

impl InvokedSkillRestoration {
    /// Extract invoked skills from a sequence of tool calls.
    ///
    /// Looks for "Skill" tool invocations and extracts skill names and arguments.
    /// Returns a list of unique skills with their most recent invocation turn.
    pub fn from_tool_calls(
        tool_calls: &[(String, serde_json::Value, i32)], // (name, input, turn_number)
    ) -> Vec<Self> {
        use std::collections::HashMap;

        // Track skills by name, keeping the most recent invocation
        let mut skills: HashMap<String, Self> = HashMap::new();

        for (name, input, turn_number) in tool_calls {
            if name == "Skill" {
                // Extract skill name from input
                if let Some(skill_name) = input.get("skill").and_then(|v| v.as_str()) {
                    let args = input.get("args").and_then(|v| v.as_str()).map(String::from);

                    // Update or insert the skill, keeping the most recent invocation
                    let entry = skills
                        .entry(skill_name.to_string())
                        .or_insert_with(|| Self {
                            name: skill_name.to_string(),
                            last_invoked_turn: *turn_number,
                            args: args.clone(),
                        });

                    // Update to most recent invocation
                    if *turn_number > entry.last_invoked_turn {
                        entry.last_invoked_turn = *turn_number;
                        entry.args = args;
                    }
                }
            }
        }

        // Convert to Vec and sort by last invoked turn (most recent first)
        let mut result: Vec<Self> = skills.into_values().collect();
        result.sort_by(|a, b| b.last_invoked_turn.cmp(&a.last_invoked_turn));
        result
    }
}

// ============================================================================
// Threshold Status
// ============================================================================

/// Multi-level threshold status for context usage.
///
/// This mirrors Claude Code's `calculateThresholds()` return type, providing
/// 5 different status levels for fine-grained compaction control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThresholdStatus {
    /// Percentage of context remaining (0.0 - 1.0).
    pub percent_left: f64,
    /// Whether context usage is above the warning threshold.
    pub is_above_warning_threshold: bool,
    /// Whether context usage is above the error threshold.
    pub is_above_error_threshold: bool,
    /// Whether context usage is above the auto-compact threshold.
    pub is_above_auto_compact_threshold: bool,
    /// Whether context usage is at the hard blocking limit.
    pub is_at_blocking_limit: bool,
}

impl ThresholdStatus {
    /// Calculate threshold status from current context usage.
    ///
    /// # Arguments
    /// * `context_tokens` - Current token count in context
    /// * `available_tokens` - Maximum available tokens for the model
    /// * `config` - Compact configuration with threshold settings
    pub fn calculate(context_tokens: i32, available_tokens: i32, config: &CompactConfig) -> Self {
        if available_tokens <= 0 {
            return Self {
                percent_left: 0.0,
                is_above_warning_threshold: true,
                is_above_error_threshold: true,
                is_above_auto_compact_threshold: true,
                is_at_blocking_limit: true,
            };
        }

        let percent_left = 1.0 - (context_tokens as f64 / available_tokens as f64);
        let target = config.auto_compact_target(available_tokens);
        let warning_threshold = config.warning_threshold(target);
        let error_threshold = config.error_threshold(target);
        let blocking_limit = config.blocking_limit(available_tokens);

        Self {
            percent_left,
            is_above_warning_threshold: context_tokens >= warning_threshold,
            is_above_error_threshold: context_tokens >= error_threshold,
            is_above_auto_compact_threshold: context_tokens >= target,
            is_at_blocking_limit: context_tokens >= blocking_limit,
        }
    }

    /// Check if any compaction action is needed.
    pub fn needs_action(&self) -> bool {
        self.is_above_warning_threshold
    }

    /// Get a human-readable status description.
    pub fn status_description(&self) -> &'static str {
        if self.is_at_blocking_limit {
            "blocking"
        } else if self.is_above_auto_compact_threshold {
            "auto-compact"
        } else if self.is_above_error_threshold {
            "error"
        } else if self.is_above_warning_threshold {
            "warning"
        } else {
            "ok"
        }
    }
}

// ============================================================================
// Keep Window Calculation
// ============================================================================

/// Information about a message for keep window calculation.
#[derive(Debug, Clone)]
pub struct MessageInfo {
    /// Index in the message array.
    pub index: i32,
    /// Estimated token count.
    pub tokens: i32,
    /// Role of the message (user, assistant, tool, etc.).
    pub role: String,
    /// Whether this is a tool_use message.
    pub is_tool_use: bool,
    /// Whether this is a tool_result message.
    pub is_tool_result: bool,
    /// Tool use ID (for pairing tool_use/tool_result).
    pub tool_use_id: Option<String>,
}

/// Result of keep window calculation.
#[derive(Debug, Clone)]
pub struct KeepWindowResult {
    /// Index of the first message to keep (0-indexed from original array).
    pub keep_start_index: i32,
    /// Number of messages to keep.
    pub messages_to_keep: i32,
    /// Total tokens in the keep window.
    pub keep_tokens: i32,
    /// Number of text messages in the keep window.
    pub text_messages_kept: i32,
}

/// Calculate the starting index for messages to keep during compaction.
///
/// This implements Claude Code's `calculateKeepStartIndex()` algorithm:
/// 1. Backscan from the end of the message array
/// 2. Accumulate tokens until we meet minimum requirements
/// 3. Ensure tool_use/tool_result pairs stay together
/// 4. Don't exceed maximum token limit
///
/// # Arguments
/// * `messages` - Array of messages with token estimates
/// * `config` - Keep window configuration
///
/// # Returns
/// `KeepWindowResult` containing the start index and statistics
pub fn calculate_keep_start_index(
    messages: &[serde_json::Value],
    config: &KeepWindowConfig,
) -> KeepWindowResult {
    if messages.is_empty() {
        return KeepWindowResult {
            keep_start_index: 0,
            messages_to_keep: 0,
            keep_tokens: 0,
            text_messages_kept: 0,
        };
    }

    // Collect message info
    let infos: Vec<MessageInfo> = messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let role = msg
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let is_tool_use = role == "assistant"
                && msg
                    .get("content")
                    .map(|c| {
                        if let Some(arr) = c.as_array() {
                            arr.iter()
                                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);

            let is_tool_result = role == "tool" || role == "tool_result";

            let tool_use_id = msg
                .get("tool_use_id")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Estimate tokens from content length (~4 chars per token)
            let content_len = msg
                .get("content")
                .and_then(|v| {
                    if let Some(s) = v.as_str() {
                        Some(s.len())
                    } else {
                        v.as_array().map(|arr| {
                            arr.iter()
                                .map(|b| {
                                    b.get("text")
                                        .or_else(|| b.get("content"))
                                        .and_then(|t| t.as_str())
                                        .map_or(0, str::len)
                                })
                                .sum()
                        })
                    }
                })
                .unwrap_or(0);
            let tokens = (content_len / 4) as i32;

            MessageInfo {
                index: i as i32,
                tokens,
                role,
                is_tool_use,
                is_tool_result,
                tool_use_id,
            }
        })
        .collect();

    // Backscan from end to find keep boundary
    let mut keep_tokens = 0;
    let mut text_messages_kept = 0;
    let mut keep_start_index = infos.len() as i32;
    let mut tool_use_ids_to_include: HashSet<String> = HashSet::new();

    for info in infos.iter().rev() {
        // Check if we've met minimum requirements AND haven't exceeded max
        let meets_min_tokens = keep_tokens >= config.min_tokens;
        let meets_min_messages = text_messages_kept >= config.min_text_messages;
        let at_max_tokens = keep_tokens >= config.max_tokens;

        // Stop if we've met all minimums and hit max, UNLESS we need to include
        // a tool_use that pairs with an already-included tool_result
        if (meets_min_tokens && meets_min_messages) || at_max_tokens {
            // Check if we need to include this message for tool pairing
            if let Some(ref id) = info.tool_use_id {
                if !tool_use_ids_to_include.contains(id) {
                    // We don't need this tool message, stop here
                    break;
                }
            } else if info.is_tool_use {
                // Tool use without an ID we're looking for, stop
                break;
            } else {
                // Regular message, stop
                break;
            }
        }

        // Include this message
        keep_start_index = info.index;
        keep_tokens += info.tokens;

        // Count text messages (user or assistant without tool use)
        if (info.role == "user" || info.role == "assistant") && !info.is_tool_use {
            text_messages_kept += 1;
        }

        // Track tool_result IDs so we include their matching tool_use
        if info.is_tool_result
            && let Some(ref id) = info.tool_use_id
        {
            tool_use_ids_to_include.insert(id.clone());
        }

        // Remove tool_use ID from set when we include the tool_use
        if info.is_tool_use
            && let Some(ref id) = info.tool_use_id
        {
            tool_use_ids_to_include.remove(id);
        }
    }

    let messages_to_keep = (infos.len() as i32) - keep_start_index;

    debug!(
        keep_start_index,
        messages_to_keep,
        keep_tokens,
        text_messages_kept,
        min_tokens = config.min_tokens,
        min_messages = config.min_text_messages,
        max_tokens = config.max_tokens,
        "Keep window calculated"
    );

    KeepWindowResult {
        keep_start_index,
        messages_to_keep,
        keep_tokens,
        text_messages_kept,
    }
}

/// Map a message index back to turn count for compaction.
///
/// Given the `keep_start_index` from [`calculate_keep_start_index()`], this
/// function converts it to a turn count that can be used with the turn-based
/// `MessageHistory::apply_compaction()` method.
///
/// The cocode-rs architecture uses a turn-based structure (`Vec<Turn>`) where
/// each turn contains a user message, an optional assistant message, and
/// potentially multiple tool call results. This function bridges the gap
/// between the message-level keep window calculation and the turn-based
/// compaction.
///
/// # Algorithm
///
/// We count backwards through the messages to find which turn contains the
/// `keep_start_index`. Each turn produces approximately:
/// - 1 user message
/// - 1 assistant message (optional)
/// - N tool results (variable)
///
/// # Arguments
/// * `turns_len` - Total number of turns in the history
/// * `messages` - The flattened message array
/// * `keep_start_index` - The index returned by `calculate_keep_start_index()`
///
/// # Returns
/// The number of turns to keep from the end of the turn list.
pub fn map_message_index_to_keep_turns(
    turns_len: i32,
    messages: &[serde_json::Value],
    keep_start_index: i32,
) -> i32 {
    if turns_len == 0 || messages.is_empty() {
        return 0;
    }

    let total_messages = messages.len() as i32;
    if keep_start_index >= total_messages {
        return 0;
    }

    let messages_to_keep = total_messages - keep_start_index;

    // Count messages per turn to get a more accurate estimate
    // Average ~3 messages per turn (user + assistant + avg tool results)
    // But ensure we keep at least 1 turn and don't exceed total turns
    let avg_messages_per_turn = (total_messages as f64 / turns_len as f64).max(1.0);
    let estimated_turns = (messages_to_keep as f64 / avg_messages_per_turn).ceil() as i32;

    // Clamp to valid range: at least 1, at most turns_len
    estimated_turns.clamp(1, turns_len)
}

// ============================================================================
// Micro-Compact Execution
// ============================================================================

/// Result of a micro-compact operation.
#[derive(Debug, Clone, Default)]
pub struct MicroCompactResult {
    /// Number of tool results that were compacted.
    pub compacted_count: i32,
    /// Total tokens saved by compaction.
    pub tokens_saved: i32,
    /// Tool use IDs that were compacted.
    pub compacted_ids: Vec<String>,
    /// UUIDs of memory attachments that were cleared.
    pub cleared_memory_uuids: Vec<String>,
    /// Persisted tool results with full metadata.
    pub persisted_results: Vec<PersistedToolResult>,
    /// File paths that were persisted (legacy compatibility).
    pub persisted_files: Vec<PathBuf>,
    /// File paths from Read tool results that were compacted.
    ///
    /// The caller can use this to clean up file state tracking (readFileState).
    pub cleared_file_paths: Vec<PathBuf>,
    /// Token breakdown for telemetry.
    pub token_breakdown: Option<TokenBreakdown>,
    /// Trigger type for this compaction.
    pub trigger: CompactTrigger,
}

/// Information about a tool result candidate for micro-compaction.
#[derive(Debug, Clone)]
pub struct ToolResultCandidate {
    /// Index in the message array.
    pub index: i32,
    /// Tool use ID (from the tool_use_id field).
    pub tool_use_id: Option<String>,
    /// Tool name (e.g., "Read", "Bash").
    pub tool_name: Option<String>,
    /// Estimated token count of the content.
    pub token_count: i32,
    /// Whether this is a compactable tool.
    pub is_compactable: bool,
}

/// Execute micro-compaction on a message history.
///
/// This implements the 7-phase micro-compact algorithm:
/// 1. Collect tool_use IDs and token counts
/// 2. Determine which tool results need compaction (keep recent N)
/// 3. Check thresholds (warning threshold + minimum savings)
/// 4. Memory attachment cleanup (placeholder - returns empty)
/// 5. Content replacement
/// 6. readFileState cleanup (placeholder)
/// 7. State update and return
///
/// # Arguments
/// * `messages` - Mutable message history (will be modified in place)
/// * `context_tokens` - Current token count
/// * `available_tokens` - Maximum available tokens
/// * `config` - Compact configuration
/// * `persist_dir` - Optional directory to persist large results
///
/// # Returns
/// Result of the micro-compaction operation, or None if no compaction was needed.
pub fn execute_micro_compact(
    messages: &mut [serde_json::Value],
    context_tokens: i32,
    available_tokens: i32,
    config: &CompactConfig,
    persist_dir: Option<&PathBuf>,
) -> Option<MicroCompactResult> {
    if !config.is_micro_compact_enabled() {
        debug!("Micro-compact disabled");
        return None;
    }

    // Phase 1: Collect tool_use IDs and token counts
    let candidates = collect_tool_result_candidates(messages);
    if candidates.is_empty() {
        debug!("No tool result candidates for micro-compaction");
        return None;
    }

    // Phase 2: Determine which tool results to compact (keep recent N)
    let recent_to_keep = config.recent_tool_results_to_keep as usize;
    let compactable_candidates: Vec<_> = candidates.iter().filter(|c| c.is_compactable).collect();

    if compactable_candidates.len() <= recent_to_keep {
        debug!(
            count = compactable_candidates.len(),
            keep = recent_to_keep,
            "Not enough compactable candidates"
        );
        return None;
    }

    // Candidates to compact are all except the most recent N
    let to_compact_count = compactable_candidates.len() - recent_to_keep;
    let candidates_to_compact: Vec<_> = compactable_candidates
        .iter()
        .take(to_compact_count)
        .collect();

    // Phase 3: Check thresholds
    let status = ThresholdStatus::calculate(context_tokens, available_tokens, config);
    let potential_savings: i32 = candidates_to_compact.iter().map(|c| c.token_count).sum();

    if !status.is_above_warning_threshold {
        debug!(
            status = status.status_description(),
            "Below warning threshold, skipping micro-compact"
        );
        return None;
    }

    if potential_savings < config.micro_compact_min_savings {
        debug!(
            potential_savings,
            min_savings = config.micro_compact_min_savings,
            "Potential savings below minimum threshold"
        );
        return None;
    }

    info!(
        candidates = to_compact_count,
        potential_savings,
        status = status.status_description(),
        "Starting micro-compaction"
    );

    // Phase 4: Memory attachment cleanup
    // Track memory attachments that need to be cleared to prevent duplication
    let mut cleared_memory_uuids: Vec<String> = Vec::new();
    for msg in messages.iter() {
        // Check for memory attachment type messages
        if let Some(msg_type) = msg.get("type").and_then(|t| t.as_str())
            && msg_type == "attachment"
            && let Some(attachment) = msg.get("attachment")
            && let Some(att_type) = attachment.get("type").and_then(|t| t.as_str())
            && att_type == "memory"
        {
            // Extract UUID and mark for clearing
            if let Some(uuid) = msg.get("uuid").and_then(|u| u.as_str())
                && !cleared_memory_uuids.contains(&uuid.to_string())
            {
                cleared_memory_uuids.push(uuid.to_string());
            }
        }
    }

    // Phase 5: Content replacement
    let mut result = MicroCompactResult::default();
    result.trigger = CompactTrigger::Auto; // Micro-compact is always auto-triggered

    for candidate in candidates_to_compact {
        let msg = &mut messages[candidate.index as usize];

        // Get original content for potential persistence
        let original_content = msg
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let original_size = original_content.len() as i64;

        // Phase 6 prep: Track file paths from Read tool results for readFileState cleanup
        // The caller can use these paths to update their file state tracking
        if candidate.tool_name.as_deref() == Some("Read") {
            // Try to extract file path from the tool input or from the message
            // Messages may have path in different locations depending on format
            if let Some(path) = msg
                .get("file_path")
                .or_else(|| msg.get("path"))
                .or_else(|| {
                    msg.get("input")
                        .and_then(|i| i.get("file_path").or_else(|| i.get("path")))
                })
                .and_then(|v| v.as_str())
            {
                result.cleared_file_paths.push(PathBuf::from(path));
            }
        }

        // Persist large results if directory provided and content is large enough
        // Large is defined as > 1000 tokens (approximately 4000 chars)
        let should_persist = original_content.len() > 4000 && persist_dir.is_some();

        let replacement_content = if let (Some(dir), true) = (persist_dir, should_persist) {
            if let Some(ref tool_use_id) = candidate.tool_use_id {
                let file_path = dir.join(format!("temp/tool-results/{tool_use_id}.txt"));
                if let Some(parent) = file_path.parent() {
                    match std::fs::create_dir_all(parent) {
                        Ok(()) => {
                            if std::fs::write(&file_path, &original_content).is_ok() {
                                // Create persisted result metadata
                                let persisted = PersistedToolResult {
                                    path: file_path.clone(),
                                    original_size,
                                    original_tokens: candidate.token_count,
                                    tool_use_id: tool_use_id.clone(),
                                };
                                let xml_ref = persisted.to_xml_reference();
                                result.persisted_results.push(persisted);
                                result.persisted_files.push(file_path);
                                xml_ref
                            } else {
                                warn!(
                                    tool_use_id,
                                    path = ?file_path,
                                    "Failed to persist tool result"
                                );
                                generate_preview(&original_content)
                            }
                        }
                        Err(e) => {
                            warn!(
                                tool_use_id,
                                path = ?parent,
                                error = %e,
                                "Failed to create directory for tool result"
                            );
                            generate_preview(&original_content)
                        }
                    }
                } else {
                    generate_preview(&original_content)
                }
            } else {
                generate_preview(&original_content)
            }
        } else {
            generate_preview(&original_content)
        };

        // Replace content
        if let Some(content) = msg.get_mut("content") {
            *content = serde_json::Value::String(replacement_content);
        }

        result.compacted_count += 1;
        result.tokens_saved += candidate.token_count;
        if let Some(ref id) = candidate.tool_use_id {
            result.compacted_ids.push(id.clone());
        }
    }

    // Phase 6: readFileState cleanup
    // File paths are now tracked in result.cleared_file_paths
    // The caller should use these to update their FileTracker state

    // Phase 7: Return result
    result.cleared_memory_uuids = cleared_memory_uuids;

    info!(
        compacted = result.compacted_count,
        tokens_saved = result.tokens_saved,
        "Micro-compaction complete"
    );

    Some(result)
}

/// Generate a preview of content with the cleared marker.
///
/// If the content is longer than `CONTENT_PREVIEW_LENGTH`, it is truncated
/// and the cleared marker is appended. Otherwise, just the marker is returned.
fn generate_preview(original_content: &str) -> String {
    if original_content.len() > CONTENT_PREVIEW_LENGTH {
        format!(
            "{}...\n\n{}",
            &original_content[..CONTENT_PREVIEW_LENGTH],
            CLEARED_CONTENT_MARKER
        )
    } else {
        CLEARED_CONTENT_MARKER.to_string()
    }
}

/// Collect information about all tool result messages.
fn collect_tool_result_candidates(messages: &[serde_json::Value]) -> Vec<ToolResultCandidate> {
    let mut candidates = Vec::new();

    for (i, msg) in messages.iter().enumerate() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");

        // Check for tool result messages (both "tool" and "tool_result" roles)
        if role != "tool" && role != "tool_result" {
            continue;
        }

        // Get tool use ID
        let tool_use_id = msg
            .get("tool_use_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Get tool name from the message or from a sibling tool_use message
        let tool_name = msg.get("name").and_then(|v| v.as_str()).map(String::from);

        // Estimate token count from content
        let content_len = msg
            .get("content")
            .and_then(|v| v.as_str())
            .map_or(0, str::len);
        let token_count = (content_len / 4) as i32;

        // Check if this tool is compactable
        let is_compactable = tool_name
            .as_deref()
            .map(|n| COMPACTABLE_TOOLS.contains(n))
            .unwrap_or(false);

        candidates.push(ToolResultCandidate {
            index: i as i32,
            tool_use_id,
            tool_name,
            token_count,
            is_compactable,
        });
    }

    candidates
}

// ============================================================================
// Full Compact Prompt Building
// ============================================================================

/// Build the 9-section compact instructions prompt.
///
/// This generates the system prompt used for LLM-based full compaction,
/// instructing the model to summarize the conversation history.
///
/// The 9 sections are:
/// 1. Summary purpose and scope
/// 2. Key decisions and outcomes
/// 3. Code changes made
/// 4. Files modified
/// 5. Errors encountered and resolutions
/// 6. User preferences learned
/// 7. Pending tasks and next steps
/// 8. Important context to preserve
/// 9. Format instructions
pub fn build_compact_instructions(max_output_tokens: i32) -> String {
    format!(
        r#"You are summarizing a conversation between a user and an AI coding assistant. Create a comprehensive summary that preserves all important context needed to continue the conversation.

## Instructions

Generate a summary covering these 9 sections:

### 1. Summary Purpose and Scope
Briefly describe what the conversation was about and the main goals.

### 2. Key Decisions and Outcomes
List the important decisions made and their outcomes. Include:
- Technical choices (libraries, patterns, architectures)
- User approvals or rejections
- Final conclusions reached

### 3. Code Changes Made
Summarize the code that was written or modified:
- New files created
- Functions or classes added/modified
- Key implementation details

### 4. Files Modified
List all files that were read, created, or modified, with brief notes on changes.

### 5. Errors Encountered and Resolutions
Document any errors or issues that came up and how they were resolved.

### 6. User Preferences Learned
Note any user preferences or patterns observed:
- Coding style preferences
- Tool usage patterns
- Communication preferences

### 7. Pending Tasks and Next Steps
List any incomplete work or planned next steps.

### 8. Important Context to Preserve
Include any other context critical for continuing the conversation:
- Environment details
- Dependencies or constraints
- Assumptions made

### 9. Format
- Use markdown formatting
- Be concise but complete
- Maximum {max_output_tokens} tokens
- Prioritize information needed to continue the work

Begin your summary now:"#
    )
}

/// Build a context restoration message that includes task status.
pub fn format_restoration_with_tasks(
    restoration: &ContextRestoration,
    tasks: Option<&TaskStatusRestoration>,
) -> String {
    let mut parts = Vec::new();

    if let Some(plan) = &restoration.plan {
        parts.push(format!("<plan_context>\n{plan}\n</plan_context>"));
    }

    if let Some(todos) = &restoration.todos {
        parts.push(format!("<todo_list>\n{todos}\n</todo_list>"));
    }

    // Add task status if present
    if let Some(task_status) = tasks
        && !task_status.tasks.is_empty()
    {
        let tasks_str = task_status
            .tasks
            .iter()
            .map(|t| {
                let owner = t.owner.as_deref().unwrap_or("unassigned");
                format!("- [{}] {} ({}): {}", t.status, t.id, owner, t.subject)
            })
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!("<task_status>\n{tasks_str}\n</task_status>"));
    }

    if !restoration.skills.is_empty() {
        parts.push(format!(
            "<active_skills>\n{}\n</active_skills>",
            restoration.skills.join("\n")
        ));
    }

    for file in &restoration.files {
        parts.push(format!(
            "<file path=\"{}\">\n{}\n</file>",
            file.path.display(),
            file.content
        ));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(
            "<restored_context>\n{}\n</restored_context>",
            parts.join("\n\n")
        )
    }
}

/// Determine whether compaction should be triggered.
///
/// Returns `true` when the ratio of `context_tokens` to `max_tokens` meets or
/// exceeds the configured `threshold`.
pub fn should_compact(context_tokens: i32, max_tokens: i32, threshold: f64) -> bool {
    if max_tokens <= 0 {
        return false;
    }
    let usage = context_tokens as f64 / max_tokens as f64;
    usage >= threshold
}

/// Identify message indices that are candidates for micro-compaction.
///
/// Micro-compaction targets messages with large `tool_result` content that can
/// be summarised without losing critical information. Returns a list of indices
/// (0-based) into the provided `messages` slice.
pub fn micro_compact_candidates(messages: &[serde_json::Value]) -> Vec<i32> {
    let mut candidates = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        // A message is a micro-compact candidate when it carries a tool_result
        // role and its content exceeds a reasonable size threshold.
        let is_tool_result = msg
            .get("role")
            .and_then(|v| v.as_str())
            .is_some_and(|r| r == "tool" || r == "tool_result");

        let content_len = msg
            .get("content")
            .and_then(|v| v.as_str())
            .map_or(0, str::len);

        // 2000 chars is a reasonable threshold for micro-compaction.
        if is_tool_result && content_len > 2000 {
            candidates.push(i as i32);
        }
    }
    candidates
}

/// Try to load a session memory summary (Tier 1 compaction).
///
/// Returns the cached summary if available and sufficient savings would result.
/// This is zero-cost as it doesn't call the LLM.
pub fn try_session_memory_compact(config: &SessionMemoryConfig) -> Option<SessionMemorySummary> {
    if !config.enabled {
        return None;
    }

    let path = config.summary_path.as_ref()?;

    // Try to read the summary file
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            debug!(?path, error = %e, "Failed to read session memory file");
            return None;
        }
    };

    if content.is_empty() {
        debug!(?path, "Session memory file is empty");
        return None;
    }

    // Parse the summary format
    let summary = parse_session_memory(&content)?;

    info!(
        summary_tokens = summary.token_estimate,
        last_id = ?summary.last_summarized_id,
        "Loaded session memory summary"
    );

    Some(summary)
}

/// Parsed session memory summary.
#[derive(Debug, Clone)]
pub struct SessionMemorySummary {
    /// The summary text.
    pub summary: String,
    /// Last message ID that was summarized.
    pub last_summarized_id: Option<String>,
    /// Estimated token count of the summary.
    pub token_estimate: i32,
}

/// Parse session memory content from summary.md format.
fn parse_session_memory(content: &str) -> Option<SessionMemorySummary> {
    // The summary.md format has metadata at the top:
    // ---
    // last_summarized_id: turn-123
    // ---
    // <summary content>

    let mut last_id = None;
    let mut summary_start = 0;

    // Check for YAML frontmatter
    if content.starts_with("---")
        && let Some(end) = content[3..].find("---")
    {
        let frontmatter = &content[3..3 + end];
        for line in frontmatter.lines() {
            if let Some(id) = line.strip_prefix("last_summarized_id:") {
                last_id = Some(id.trim().to_string());
            }
        }
        summary_start = 3 + end + 3;
        // Skip leading newlines
        while summary_start < content.len() && content[summary_start..].starts_with('\n') {
            summary_start += 1;
        }
    }

    let summary = content[summary_start..].trim().to_string();
    if summary.is_empty() {
        return None;
    }

    // Rough token estimate: ~4 chars per token
    let token_estimate = (summary.len() / 4) as i32;

    Some(SessionMemorySummary {
        summary,
        last_summarized_id: last_id,
        token_estimate,
    })
}

/// Write session memory summary to a file for future Tier 1 compaction.
///
/// The file format includes YAML frontmatter with metadata followed by the summary content:
///
/// ```text
/// ---
/// last_summarized_id: turn-123
/// timestamp: 1706614800000
/// ---
/// <summary content>
/// ```
///
/// # Arguments
/// * `path` - Path to the summary file (typically `~/.claude/projects/{session}/session-memory/summary.md`)
/// * `summary` - The summary content to write
/// * `last_summarized_id` - ID of the last message that was summarized
///
/// # Errors
/// Returns an IO error if the file cannot be written.
pub async fn write_session_memory(
    path: &std::path::PathBuf,
    summary: &str,
    last_summarized_id: &str,
) -> std::io::Result<()> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let content = format!(
        "---\nlast_summarized_id: {last_summarized_id}\ntimestamp: {timestamp}\n---\n{summary}"
    );

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    tokio::fs::write(path, content).await
}

/// Build context restoration items within the given token budget.
///
/// Prioritizes items by importance and fits as many as possible within budget.
pub fn build_context_restoration(
    files: Vec<FileRestoration>,
    todos: Option<String>,
    plan: Option<String>,
    skills: Vec<String>,
    budget: i32,
) -> ContextRestoration {
    build_context_restoration_with_config(
        files,
        todos,
        plan,
        skills,
        &FileRestorationConfig {
            max_files: CONTEXT_RESTORATION_MAX_FILES,
            max_tokens_per_file: cocode_protocol::DEFAULT_MAX_TOKENS_PER_FILE,
            total_token_budget: budget,
            excluded_patterns: vec![],
            sort_by_access_time: false, // Default to priority-based sorting
        },
    )
}

/// Build context restoration items with full configuration.
///
/// This is the extended version that supports file exclusion patterns,
/// access-time sorting, and per-file token limits.
///
/// # Arguments
/// * `files` - Files to potentially restore (with last_accessed timestamps)
/// * `todos` - Todo list state to restore
/// * `plan` - Plan mode state to restore
/// * `skills` - Active skills to restore
/// * `config` - File restoration configuration with exclusion rules
pub fn build_context_restoration_with_config(
    files: Vec<FileRestoration>,
    todos: Option<String>,
    plan: Option<String>,
    skills: Vec<String>,
    config: &FileRestorationConfig,
) -> ContextRestoration {
    let mut result = ContextRestoration::default();
    let mut remaining = config.total_token_budget;

    // Priority 1: Plan mode state (if active)
    if let Some(p) = plan {
        let tokens = estimate_tokens_for_text(&p);
        if tokens <= remaining {
            result.plan = Some(p);
            remaining -= tokens;
        }
    }

    // Priority 2: Todo list
    if let Some(t) = todos {
        let tokens = estimate_tokens_for_text(&t);
        if tokens <= remaining {
            result.todos = Some(t);
            remaining -= tokens;
        }
    }

    // Priority 3: Skills (typically small)
    for skill in skills {
        let tokens = estimate_tokens_for_text(&skill);
        if tokens <= remaining {
            result.skills.push(skill);
            remaining -= tokens;
        }
    }

    // Priority 4: Files (with exclusion, sorting, and limits)
    // First, filter out excluded files
    let mut eligible_files: Vec<FileRestoration> = files
        .into_iter()
        .filter(|f| {
            let path_str = f.path.to_string_lossy();
            !config.should_exclude(&path_str)
        })
        .collect();

    // Sort files: by access time if configured, otherwise by priority
    if config.sort_by_access_time {
        // Sort by last_accessed descending (most recent first)
        eligible_files.sort_by(|a, b| b.last_accessed.cmp(&a.last_accessed));
    } else {
        // Sort by priority descending (higher priority first)
        eligible_files.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    // Add files up to limits
    let mut files_added = 0;
    for mut file in eligible_files {
        if files_added >= config.max_files {
            break;
        }

        // Truncate file content if it exceeds per-file limit
        if file.tokens > config.max_tokens_per_file {
            // Calculate approximate character limit (~4 chars per token)
            let char_limit = (config.max_tokens_per_file * 4) as usize;
            if file.content.len() > char_limit {
                file.content = format!(
                    "{}...\n[Truncated: {} more tokens]",
                    &file.content[..char_limit.min(file.content.len())],
                    file.tokens - config.max_tokens_per_file
                );
                file.tokens = config.max_tokens_per_file;
            }
        }

        if file.tokens <= remaining {
            remaining -= file.tokens;
            result.files.push(file);
            files_added += 1;
        }
    }

    debug!(
        files_restored = result.files.len(),
        budget_used = config.total_token_budget - remaining,
        budget_remaining = remaining,
        "Context restoration built"
    );

    result
}

/// Estimate token count for text (rough approximation).
fn estimate_tokens_for_text(text: &str) -> i32 {
    // ~4 chars per token is a rough estimate
    (text.len() / 4) as i32
}

/// Format context restoration as a message for the conversation.
pub fn format_restoration_message(restoration: &ContextRestoration) -> String {
    let mut parts = Vec::new();

    if let Some(plan) = &restoration.plan {
        parts.push(format!("<plan_context>\n{plan}\n</plan_context>"));
    }

    if let Some(todos) = &restoration.todos {
        parts.push(format!("<todo_list>\n{todos}\n</todo_list>"));
    }

    if !restoration.skills.is_empty() {
        parts.push(format!(
            "<active_skills>\n{}\n</active_skills>",
            restoration.skills.join("\n")
        ));
    }

    for file in &restoration.files {
        parts.push(format!(
            "<file path=\"{}\">\n{}\n</file>",
            file.path.display(),
            file.content
        ));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(
            "<restored_context>\n{}\n</restored_context>",
            parts.join("\n\n")
        )
    }
}

// ============================================================================
// Summary Formatting with Transcript Reference
// ============================================================================

/// Format a compact summary with continuation message and transcript reference.
///
/// This creates a summary that:
/// 1. Indicates the session is continued from a previous conversation
/// 2. Includes the transcript path for full history reference
/// 3. Notes whether recent messages were preserved
///
/// # Arguments
/// * `summary` - The LLM-generated summary content
/// * `transcript_path` - Optional path to the full transcript file
/// * `recent_messages_preserved` - Whether recent messages were kept verbatim
/// * `pre_tokens` - Token count before compaction
pub fn format_summary_with_transcript(
    summary: &str,
    transcript_path: Option<&PathBuf>,
    recent_messages_preserved: bool,
    pre_tokens: i32,
) -> String {
    let mut parts = Vec::new();

    // Add continuation header
    parts.push(
        "This session is being continued from a previous conversation that was compacted to save context space.".to_string()
    );

    // Add token info
    parts.push(format!(
        "The original conversation contained approximately {pre_tokens} tokens."
    ));

    // Add transcript path reference if available
    if let Some(path) = transcript_path {
        parts.push(format!(
            "\nIf you need specific details from the conversation history (like exact code snippets, error messages, or content that was generated), read the full transcript at: {}",
            path.display()
        ));
    }

    // Note about preserved messages
    if recent_messages_preserved {
        parts.push("\nRecent messages are preserved verbatim below the summary.".to_string());
    }

    // Add separator and summary
    parts.push("\n---\n".to_string());
    parts.push(summary.to_string());

    parts.join("\n")
}

/// Create an invoked skills attachment for restoration after compaction.
///
/// This creates a formatted attachment listing recently invoked skills
/// that should be restored to maintain context.
pub fn create_invoked_skills_attachment(
    invoked_skills: &[InvokedSkillRestoration],
) -> Option<String> {
    if invoked_skills.is_empty() {
        return None;
    }

    let skills_list: Vec<String> = invoked_skills
        .iter()
        .map(|s| {
            if let Some(args) = &s.args {
                format!(
                    "- {} (args: {}, last used turn {})",
                    s.name, args, s.last_invoked_turn
                )
            } else {
                format!("- {} (last used turn {})", s.name, s.last_invoked_turn)
            }
        })
        .collect();

    Some(format!(
        "<invoked_skills>\nRecently invoked skills:\n{}\n</invoked_skills>",
        skills_list.join("\n")
    ))
}

/// Create a compact boundary message with metadata.
///
/// This message marks where compaction occurred and includes metadata
/// about the trigger and token counts.
pub fn create_compact_boundary_message(metadata: &CompactBoundaryMetadata) -> String {
    let mut content = "Conversation compacted.".to_string();

    content.push_str(&format!(
        "\nTrigger: {}\nTokens before: {}",
        metadata.trigger.as_str(),
        metadata.pre_tokens
    ));

    if let Some(post) = metadata.post_tokens {
        content.push_str(&format!("\nTokens after: {post}"));
    }

    if let Some(path) = &metadata.transcript_path {
        content.push_str(&format!("\nFull transcript: {}", path.display()));
    }

    if metadata.recent_messages_preserved {
        content.push_str("\nRecent messages preserved verbatim.");
    }

    content
}

/// Wrap hook additional context as a formatted message.
///
/// Creates the hook_additional_context message format used for
/// post-compact SessionStart hook results.
pub fn wrap_hook_additional_context(contexts: &[HookAdditionalContext]) -> Option<String> {
    if contexts.is_empty() {
        return None;
    }

    let formatted: Vec<String> = contexts
        .iter()
        .filter(|c| !c.suppress_output)
        .map(|c| {
            format!(
                "<hook_context name=\"{}\">\n{}\n</hook_context>",
                c.hook_name, c.content
            )
        })
        .collect();

    if formatted.is_empty() {
        return None;
    }

    Some(format!(
        "<hook_additional_context>\n{}\n</hook_additional_context>",
        formatted.join("\n\n")
    ))
}

/// Build token breakdown for telemetry.
///
/// Analyzes messages to calculate token distribution by category.
pub fn build_token_breakdown(messages: &[serde_json::Value]) -> TokenBreakdown {
    let mut breakdown = TokenBreakdown::default();
    let mut tool_request_tokens: HashMap<String, i32> = HashMap::new();
    let mut tool_result_tokens: HashMap<String, i32> = HashMap::new();

    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content_len = msg
            .get("content")
            .and_then(|v| v.as_str())
            .map_or(0, str::len);
        let tokens = (content_len / 4) as i32;

        breakdown.total_tokens += tokens;

        match role {
            "user" | "human" => {
                breakdown.human_message_tokens += tokens;
            }
            "assistant" => {
                breakdown.assistant_message_tokens += tokens;
            }
            "tool" | "tool_result" => {
                // Track by tool name
                if let Some(name) = msg.get("name").and_then(|v| v.as_str()) {
                    *tool_result_tokens.entry(name.to_string()).or_insert(0) += tokens;
                }
                breakdown.local_command_output_tokens += tokens;
            }
            _ => {}
        }

        // Check for tool use blocks in assistant messages
        if role == "assistant"
            && let Some(content) = msg.get("content")
            && let Some(arr) = content.as_array()
        {
            for block in arr {
                if let Some(block_type) = block.get("type").and_then(|t| t.as_str())
                    && block_type == "tool_use"
                    && let Some(name) = block.get("name").and_then(|n| n.as_str())
                {
                    let input_len = block.get("input").map(|i| i.to_string().len()).unwrap_or(0);
                    let input_tokens = (input_len / 4) as i32;
                    *tool_request_tokens.entry(name.to_string()).or_insert(0) += input_tokens;
                }
            }
        }
    }

    // Calculate percentages
    if breakdown.total_tokens > 0 {
        breakdown.human_message_pct =
            breakdown.human_message_tokens as f64 / breakdown.total_tokens as f64 * 100.0;
        breakdown.assistant_message_pct =
            breakdown.assistant_message_tokens as f64 / breakdown.total_tokens as f64 * 100.0;
        breakdown.local_command_output_pct =
            breakdown.local_command_output_tokens as f64 / breakdown.total_tokens as f64 * 100.0;
    }

    breakdown.tool_request_tokens = tool_request_tokens;
    breakdown.tool_result_tokens = tool_result_tokens;

    breakdown
}

#[cfg(test)]
#[path = "compaction.test.rs"]
mod tests;
