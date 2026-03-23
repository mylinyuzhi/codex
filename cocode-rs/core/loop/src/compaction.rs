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
//! The micro-compact algorithm runs in 8 phases:
//! 1. Collect tool_use IDs and token counts
//! 2. Determine which tool results need compaction (keep recent N)
//! 3. Check thresholds (warning threshold + minimum savings)
//! 4. Memory attachment cleanup
//! 5. Content replacement (persist or clear marker)
//!    - 5.5: Image clearing for completed exchanges (preserves pending images)
//! 6. readFileState cleanup
//! 7. State update and return
//!
//! ## Compactable Tools
//!
//! Only certain tools have results that can be safely compacted:
//! - Read, Bash, Grep, Glob - file/command output
//! - WebSearch, WebFetch - web content
//! - Edit, Write - file operation confirmations

use cocode_message::MessageHistory;
use cocode_protocol::ToolName;
use cocode_tools::FileReadState;
use cocode_tools::FileTracker;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use tracing::debug;
use tracing::info;

// Re-export commonly used types and constants from protocol for convenience
pub use cocode_protocol::CompactBoundaryMetadata;
pub use cocode_protocol::CompactConfig;
pub use cocode_protocol::CompactTelemetry;
pub use cocode_protocol::CompactTrigger;
pub use cocode_protocol::CompactedLargeFileRef;
pub use cocode_protocol::FileRestorationConfig;
pub use cocode_protocol::HookAdditionalContext;
pub use cocode_protocol::KeepWindowConfig;
pub use cocode_protocol::MemoryAttachment;
pub use cocode_protocol::PersistedToolResult;
pub use cocode_protocol::TokenBreakdown;

// ============================================================================
// File Tracker LRU Limits (from Claude Code v2.1.38)
// ============================================================================

/// Maximum number of entries in the file tracker LRU cache.
pub const LRU_MAX_ENTRIES: usize = 100;

/// Maximum total size in bytes for file tracker content (~25MB).
pub const LRU_MAX_SIZE_BYTES: usize = 26_214_400;

// ============================================================================
// Compactable Tools
// ============================================================================

/// Tools whose results can be safely micro-compacted.
///
/// These tools produce output that can be replaced with a placeholder or
/// persisted to disk without losing critical conversation context.
pub static COMPACTABLE_TOOLS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    use cocode_protocol::ToolName;
    HashSet::from([
        ToolName::Read.as_str(),      // File content - can be re-read
        ToolName::Bash.as_str(),      // Command output - typically verbose
        ToolName::Grep.as_str(),      // Search results - can be re-run
        ToolName::Glob.as_str(),      // File listings - can be re-run
        ToolName::WebSearch.as_str(), // Search results - ephemeral
        ToolName::WebFetch.as_str(),  // Web content - can be re-fetched
        ToolName::Edit.as_str(),      // Edit confirmation - minimal info loss
        ToolName::Write.as_str(),     // Write confirmation - minimal info loss
    ])
});

/// Marker text used to replace cleared tool result content.
pub const CLEARED_CONTENT_MARKER: &str = "[Old tool result content cleared]";

/// Maximum characters to keep as a preview when clearing content.
pub const CONTENT_PREVIEW_LENGTH: usize = 2000;

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
    /// References to large files that were compacted (content removed but reference kept).
    ///
    /// When files exceed the restoration size limit, they're added here so the
    /// model knows the file was read and can re-read it if needed.
    pub compacted_large_files: Vec<CompactedLargeFileRef>,
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
            if name == cocode_protocol::ToolName::TodoWrite.as_str()
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
            if name == cocode_protocol::ToolName::Skill.as_str() {
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

/// Estimate token count from a message's content.
///
/// Extracts text length from string or array content blocks and converts
/// to approximate token count using the canonical `estimate_text_tokens()`.
fn estimate_message_tokens(msg: &serde_json::Value) -> i32 {
    let content = msg
        .get("content")
        .and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|b| {
                            b.get("text")
                                .or_else(|| b.get("content"))
                                .and_then(|t| t.as_str())
                        })
                        .collect::<Vec<_>>()
                        .join("")
                })
            }
        })
        .unwrap_or_default();
    cocode_protocol::estimate_text_tokens(&content)
}

/// Check if a message is a compact boundary marker.
///
/// Boundary messages are user messages whose content starts with
/// "Conversation compacted." — they mark where a previous compaction occurred.
fn is_compact_boundary_message(msg: &serde_json::Value) -> bool {
    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
    if role != "user" {
        return false;
    }
    msg.get("content")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.starts_with("Conversation compacted."))
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

            let tokens = estimate_message_tokens(msg);

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
// Session Memory Boundary Finding
// ============================================================================

/// Result of session memory boundary calculation.
#[derive(Debug, Clone, Default)]
pub struct SessionMemoryBoundaryResult {
    /// Index of the first message to keep (0-indexed).
    pub keep_start_index: i32,
    /// Number of messages in the keep window.
    pub messages_to_keep: i32,
    /// Total tokens in the keep window.
    pub keep_tokens: i32,
    /// Number of text messages kept.
    pub text_messages_kept: i32,
}

impl From<KeepWindowResult> for SessionMemoryBoundaryResult {
    fn from(r: KeepWindowResult) -> Self {
        Self {
            keep_start_index: r.keep_start_index,
            messages_to_keep: r.messages_to_keep,
            keep_tokens: r.keep_tokens,
            text_messages_kept: r.text_messages_kept,
        }
    }
}

/// Find the compaction boundary for session memory compaction.
///
/// This implements an anchor-point-based boundary finding algorithm aligned
/// with Claude Code's `findCompactionBoundary`:
///
/// **Phase 1**: Start from the anchor (last_summarized_id) and count forward,
/// checking if we've accumulated enough messages/tokens after the anchor.
///
/// **Phase 2**: If the anchor exists but content after it is insufficient,
/// walk backward from the anchor to include more messages, stopping at the
/// last compact boundary message (never crossing a previous compaction point).
///
/// **Phase 3 (fallback)**: If anchor is not found or not provided, fall back
/// to the generic `calculate_keep_start_index()` algorithm.
///
/// After determining the raw boundary, `adjust_boundaries_for_tools()` is
/// called to ensure tool_use/tool_result pairs are not split.
///
/// # Arguments
/// * `messages` - Array of messages with token estimates
/// * `config` - Keep window configuration (min/max tokens, min text messages)
/// * `last_summarized_id` - ID of the last message that was summarized (anchor point)
pub fn find_session_memory_boundary(
    messages: &[serde_json::Value],
    config: &cocode_protocol::KeepWindowConfig,
    last_summarized_id: Option<&str>,
) -> SessionMemoryBoundaryResult {
    if messages.is_empty() {
        return SessionMemoryBoundaryResult::default();
    }

    // Phase 1: Try anchor-based boundary finding
    if let Some(anchor_id) = last_summarized_id {
        // Find the anchor message index
        let anchor_index = messages.iter().position(|msg| {
            msg.get("turn_id")
                .or_else(|| msg.get("id"))
                .and_then(|v| v.as_str())
                == Some(anchor_id)
        });

        if let Some(anchor_idx) = anchor_index {
            // Count tokens and messages AFTER the anchor (anchor was already summarized)
            let mut tokens_after_anchor = 0i32;
            let mut text_messages_after = 0i32;

            for msg in &messages[anchor_idx + 1..] {
                tokens_after_anchor += estimate_message_tokens(msg);

                let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role == "user" || role == "assistant" {
                    let has_tool_use =
                        msg.get("content")
                            .and_then(|c| c.as_array())
                            .is_some_and(|arr| {
                                arr.iter().any(|b| {
                                    b.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                                })
                            });
                    if !has_tool_use {
                        text_messages_after += 1;
                    }
                }
            }

            // If there's enough content after the anchor, use it as the keep boundary
            if tokens_after_anchor >= config.min_tokens
                && text_messages_after >= config.min_text_messages
            {
                let raw_start = (anchor_idx + 1) as i32;
                let adjusted = adjust_boundaries_for_tools(messages, raw_start);
                let keep_start = adjusted.min(messages.len() as i32);
                let messages_to_keep = messages.len() as i32 - keep_start;

                // Count actual tokens in the final keep window
                let mut actual_tokens = 0i32;
                let mut actual_text_msgs = 0i32;
                for msg in &messages[keep_start as usize..] {
                    actual_tokens += estimate_message_tokens(msg);
                    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
                    if role == "user" || role == "assistant" {
                        actual_text_msgs += 1;
                    }
                }

                debug!(
                    anchor_idx,
                    keep_start,
                    messages_to_keep,
                    tokens = actual_tokens,
                    text_messages = actual_text_msgs,
                    "Session memory boundary found via anchor"
                );

                return SessionMemoryBoundaryResult {
                    keep_start_index: keep_start,
                    messages_to_keep,
                    keep_tokens: actual_tokens,
                    text_messages_kept: actual_text_msgs,
                };
            }

            // Phase 2: Content after anchor is insufficient — walk backward from
            // anchor to include more messages. Stop at the last compact boundary
            // message to never cross a previous compaction point.
            let last_boundary_index = {
                let mut boundary = 0usize;
                for i in (0..messages.len()).rev() {
                    if is_compact_boundary_message(&messages[i]) {
                        boundary = i + 1;
                        break;
                    }
                }
                boundary
            };

            let mut start_index = anchor_idx + 1;
            let mut total_tokens = tokens_after_anchor;
            let mut text_block_count = text_messages_after;

            for i in (last_boundary_index..=anchor_idx).rev() {
                let msg = &messages[i];
                total_tokens += estimate_message_tokens(msg);

                let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role == "user" || role == "assistant" {
                    let has_tool_use =
                        msg.get("content")
                            .and_then(|c| c.as_array())
                            .is_some_and(|arr| {
                                arr.iter().any(|b| {
                                    b.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                                })
                            });
                    if !has_tool_use {
                        text_block_count += 1;
                    }
                }

                start_index = i;

                // Stop if we've hit max_tokens
                if total_tokens >= config.max_tokens {
                    break;
                }
                // Stop if we've met both minimums
                if total_tokens >= config.min_tokens && text_block_count >= config.min_text_messages
                {
                    break;
                }
            }

            let raw_start = start_index as i32;
            let adjusted = adjust_boundaries_for_tools(messages, raw_start);
            let keep_start = adjusted.min(messages.len() as i32);
            let messages_to_keep = messages.len() as i32 - keep_start;

            // Recount tokens in the final window
            let mut actual_tokens = 0i32;
            let mut actual_text_msgs = 0i32;
            for msg in &messages[keep_start as usize..] {
                actual_tokens += estimate_message_tokens(msg);
                let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role == "user" || role == "assistant" {
                    actual_text_msgs += 1;
                }
            }

            debug!(
                anchor_idx,
                start_index,
                keep_start,
                messages_to_keep,
                tokens = actual_tokens,
                text_messages = actual_text_msgs,
                "Session memory boundary found via backward walk from anchor"
            );

            return SessionMemoryBoundaryResult {
                keep_start_index: keep_start,
                messages_to_keep,
                keep_tokens: actual_tokens,
                text_messages_kept: actual_text_msgs,
            };
        }
    }

    // Phase 3: No anchor or anchor not found — fall back to generic calculation
    calculate_keep_start_index(messages, config).into()
}

/// Adjust a raw keep boundary to avoid splitting tool_use/tool_result pairs.
///
/// Walks backward from `raw_start` to ensure that if a tool_result message
/// is included, its corresponding tool_use message is also included.
///
/// # Arguments
/// * `messages` - The message array
/// * `raw_start` - The raw start index to adjust
///
/// # Returns
/// The adjusted start index (may be <= raw_start).
fn adjust_boundaries_for_tools(messages: &[serde_json::Value], raw_start: i32) -> i32 {
    if raw_start <= 0 || messages.is_empty() {
        return raw_start;
    }

    let start = raw_start as usize;
    if start >= messages.len() {
        return raw_start;
    }

    // Collect tool_use IDs referenced by tool_result messages in the keep window
    let mut needed_tool_use_ids: HashSet<String> = HashSet::new();
    for msg in &messages[start..] {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if (role == "tool" || role == "tool_result")
            && let Some(id) = msg.get("tool_use_id").and_then(|v| v.as_str())
        {
            needed_tool_use_ids.insert(id.to_string());
        }
    }

    if needed_tool_use_ids.is_empty() {
        return raw_start;
    }

    // Remove IDs that are already satisfied within the keep window
    for msg in &messages[start..] {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "assistant"
            && let Some(content) = msg.get("content").and_then(|c| c.as_array())
        {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                    && let Some(id) = block.get("id").and_then(|v| v.as_str())
                {
                    needed_tool_use_ids.remove(id);
                }
            }
        }
    }

    if needed_tool_use_ids.is_empty() {
        return raw_start;
    }

    // Walk backward from raw_start to find the tool_use messages
    let mut adjusted_start = raw_start;
    for i in (0..start).rev() {
        let msg = &messages[i];
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "assistant"
            && let Some(content) = msg.get("content").and_then(|c| c.as_array())
        {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                    && let Some(id) = block.get("id").and_then(|v| v.as_str())
                    && needed_tool_use_ids.remove(id)
                {
                    adjusted_start = i as i32;
                }
            }
        }

        if needed_tool_use_ids.is_empty() {
            break;
        }
    }

    debug!(
        raw_start,
        adjusted_start, "Adjusted boundary for tool pairing"
    );
    adjusted_start
}

/// Collect file paths to keep during compaction.
///
/// This implements Claude Code's `collectFilesToKeep` (Ua4):
/// - Get files from recent turns (those being kept after compaction)
/// - Return a set of paths that should be preserved in the FileTracker
///
/// This ensures that recently accessed files remain in the tracker's cache
/// even after older tool results are micro-compacted.
///
/// # Arguments
/// * `history` - The message history to extract recent files from
/// * `keep_recent_turns` - Number of recent turns to preserve
///
/// # Returns
/// A HashSet of file paths that should be preserved in the FileTracker.
pub fn collect_files_to_keep(history: &MessageHistory, keep_recent_turns: i32) -> HashSet<PathBuf> {
    let mut files_to_keep = HashSet::new();
    let total_turns = history.turns().len();

    // Calculate which turns to keep (the most recent N)
    let keep_from_turn = total_turns.saturating_sub(keep_recent_turns as usize);

    // Collect file paths from recent turns
    for turn in history.turns().iter().skip(keep_from_turn) {
        for tool_call in &turn.tool_calls {
            // Extract file paths from Read tool calls
            if tool_call.name == ToolName::Read.as_str()
                && let Some(file_path) = tool_call.input.get("file_path").and_then(|v| v.as_str())
            {
                files_to_keep.insert(PathBuf::from(file_path));
            }
            // Also track Edit and Write tools as they modify files
            if (tool_call.name == ToolName::Edit.as_str()
                || tool_call.name == ToolName::Write.as_str())
                && let Some(file_path) = tool_call.input.get("file_path").and_then(|v| v.as_str())
            {
                files_to_keep.insert(PathBuf::from(file_path));
            }
        }
    }

    debug!(
        total_turns,
        keep_recent_turns,
        files_to_keep = files_to_keep.len(),
        "Collected files to keep during compaction"
    );

    files_to_keep
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

/// Try to load a session memory summary (Tier 1 compaction).
///
/// Returns the cached summary if available and sufficient savings would result.
/// This is zero-cost as it doesn't call the LLM.
///
/// Uses `CompactConfig.enable_sm_compact` as the gate and `CompactConfig.summary_path`
/// as the file location, consolidating what was previously split across
/// `CompactConfig` and a separate `SessionMemoryConfig`.
pub fn try_session_memory_compact(config: &CompactConfig) -> Option<SessionMemorySummary> {
    if !config.enable_sm_compact {
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

    if is_empty_template(&content) {
        debug!(?path, "Session memory file is an unmodified template");
        return None;
    }

    // Parse the summary format
    let mut summary = parse_session_memory(&content)?;

    // Truncate oversized sections to prevent SM notes from consuming
    // disproportionate context. Matches Claude Code's `truncateSections()`.
    summary.summary =
        truncate_sections(&summary.summary, SM_MAX_SECTION_TOKENS, SM_MAX_TOTAL_TOKENS);
    summary.token_estimate = cocode_protocol::estimate_text_tokens(&summary.summary);

    info!(
        summary_tokens = summary.token_estimate,
        last_id = ?summary.last_summarized_id,
        "Loaded session memory summary"
    );

    Some(summary)
}

/// Default per-section token limit for session memory notes.
const SM_MAX_SECTION_TOKENS: i32 = 2000;

/// Default total token limit for session memory notes.
const SM_MAX_TOTAL_TOKENS: i32 = 12000;

/// Truncate session memory sections to enforce per-section and total token limits.
///
/// Matches Claude Code's `truncateSections()`: each markdown section (delimited by
/// `### ` headers) is capped at `max_section_tokens`, and the total output is capped
/// at `max_total_tokens`. Uses [`cocode_protocol::estimate_text_tokens`] for budget checks.
pub fn truncate_sections(summary: &str, max_section_tokens: i32, max_total_tokens: i32) -> String {
    let mut result = String::with_capacity(summary.len());
    let mut total_used: i32 = 0;
    let mut current_section = String::new();
    let mut in_section = false;

    for line in summary.lines() {
        if line.starts_with("### ") || line.starts_with("## ") || line.starts_with("# ") {
            // Flush previous section
            if in_section {
                let truncated = truncate_to_token_limit(&current_section, max_section_tokens);
                let section_tokens = cocode_protocol::estimate_text_tokens(&truncated);
                if total_used + section_tokens > max_total_tokens {
                    break;
                }
                result.push_str(&truncated);
                total_used += section_tokens;
                current_section.clear();
            }
            in_section = true;
            current_section.push_str(line);
            current_section.push('\n');
        } else if in_section {
            current_section.push_str(line);
            current_section.push('\n');
        } else {
            // Content before the first header — include directly
            let line_tokens = cocode_protocol::estimate_text_tokens(line);
            if total_used + line_tokens > max_total_tokens {
                break;
            }
            result.push_str(line);
            result.push('\n');
            total_used += line_tokens;
        }
    }

    // Flush last section
    if in_section && !current_section.is_empty() {
        let truncated = truncate_to_token_limit(&current_section, max_section_tokens);
        let section_tokens = cocode_protocol::estimate_text_tokens(&truncated);
        if total_used + section_tokens <= max_total_tokens {
            result.push_str(&truncated);
        }
    }

    // Remove trailing newline to match input convention
    if result.ends_with('\n') && !summary.ends_with('\n') {
        result.pop();
    }
    result
}

/// Truncate text to fit within a token limit, appending "[truncated]" if needed.
fn truncate_to_token_limit(text: &str, max_tokens: i32) -> String {
    let tokens = cocode_protocol::estimate_text_tokens(text);
    if tokens <= max_tokens {
        return text.to_string();
    }

    // Estimate the character budget: ~3 chars per token
    let char_budget = (max_tokens as usize) * 3;
    let truncation_marker = "\n[truncated]\n";
    let usable = char_budget.saturating_sub(truncation_marker.len());

    // Find a clean break point (newline boundary)
    let break_point = text[..usable.min(text.len())]
        .rfind('\n')
        .unwrap_or(usable.min(text.len()));

    let mut result = text[..break_point].to_string();
    result.push_str(truncation_marker);
    result
}

/// Check whether a session memory summary is an unmodified template.
///
/// Returns `true` when all non-header, non-empty lines are just "N/A"
/// (or there are fewer than 3 substantive lines). This matches Claude Code's
/// `isEmptyTemplate()` check.
fn is_empty_template(summary: &str) -> bool {
    let substantive_lines = summary
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with('*'))
        .filter(|l| {
            let trimmed = l.trim();
            !trimmed.is_empty() && trimmed != "N/A" && trimmed != "---"
        })
        .count();
    substantive_lines < 3
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

    let token_estimate = cocode_protocol::estimate_text_tokens(&summary);

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
            max_files: cocode_protocol::DEFAULT_CONTEXT_RESTORE_MAX_FILES,
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
    // First, filter out excluded files and internal files
    let mut eligible_files: Vec<FileRestoration> = files
        .into_iter()
        .filter(|f| {
            let path_str = f.path.to_string_lossy();
            // Filter by exclusion patterns from config
            if config.should_exclude(&path_str) {
                return false;
            }
            // Filter out internal files (session memory, plan files, etc.)
            if is_internal_file(&f.path, "") {
                return false;
            }
            true
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
            // Calculate approximate character limit (~3 chars per token)
            let char_limit = (config.max_tokens_per_file * 3) as usize;
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

/// Estimate token count for text using the canonical formula.
fn estimate_tokens_for_text(text: &str) -> i32 {
    cocode_protocol::estimate_text_tokens(text)
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
    // P5: Track duplicate file reads
    let mut seen_read_paths: HashSet<String> = HashSet::new();

    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content_text = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let tokens = cocode_protocol::estimate_text_tokens(content_text);

        breakdown.total_tokens += tokens;

        match role {
            "user" | "human" => {
                breakdown.human_message_tokens += tokens;
            }
            "assistant" => {
                breakdown.assistant_message_tokens += tokens;
            }
            "tool" | "tool_result" => {
                let tool_name = msg.get("name").and_then(|v| v.as_str());
                // Track by tool name
                if let Some(name) = tool_name {
                    *tool_result_tokens.entry(name.to_string()).or_insert(0) += tokens;
                }
                breakdown.local_command_output_tokens += tokens;

                // P5: Detect duplicate Read file paths
                if tool_name == Some(ToolName::Read.as_str()) {
                    let file_path = msg
                        .get("file_path")
                        .or_else(|| msg.get("input").and_then(|i| i.get("file_path")))
                        .and_then(|v| v.as_str());
                    if let Some(path) = file_path
                        && !seen_read_paths.insert(path.to_string())
                    {
                        // Duplicate read
                        breakdown.duplicate_read_tokens += tokens;
                        breakdown.duplicate_read_file_count += 1;
                    }
                }
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
                    let input_text = block
                        .get("input")
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let input_tokens = cocode_protocol::estimate_text_tokens(&input_text);
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

// ============================================================================
// File State Rebuild from Message History
// ============================================================================

/// Extract file read state from message history.
///
/// This implements Claude Code's approach where file state is derived
/// from actual tool calls in messages, ensuring consistency and
/// enabling automatic recovery after compaction.
///
/// # Arguments
/// * `history` - The message history to extract file state from
/// * `cwd` - Current working directory for resolving relative paths
/// * `max_entries` - Maximum number of entries to track (LRU limit)
///
/// # Returns
/// A `FileTracker` populated with file state extracted from Read and Edit tool calls.
pub fn build_file_read_state(
    history: &MessageHistory,
    cwd: &Path,
    max_entries: usize,
) -> FileTracker {
    let tracker = FileTracker::new();

    // Maps to track tool_use -> tool_result correlation
    let mut read_tool_map: HashMap<String, PathBuf> = HashMap::new();
    let mut edit_tool_map: HashMap<String, (PathBuf, String)> = HashMap::new();

    // Phase 1: Collect tool_use IDs from assistant messages
    for turn in history.turns() {
        if let Some(_msg) = &turn.assistant_message {
            for tool_call in &turn.tool_calls {
                if tool_call.name == ToolName::Read.as_str() {
                    // Only track full reads (no offset/limit)
                    if let Some(file_path) =
                        tool_call.input.get("file_path").and_then(|v| v.as_str())
                    {
                        let has_offset = tool_call.input.get("offset").is_some();
                        let has_limit = tool_call.input.get("limit").is_some();
                        if !has_offset && !has_limit {
                            let abs_path = cwd.join(file_path);
                            read_tool_map.insert(tool_call.call_id.clone(), abs_path);
                        }
                    }
                } else if tool_call.name == ToolName::Edit.as_str() {
                    // Track Edit tools - we need the new_string content
                    if let (Some(file_path), Some(content)) = (
                        tool_call.input.get("file_path").and_then(|v| v.as_str()),
                        tool_call.input.get("new_string").and_then(|v| v.as_str()),
                    ) {
                        let abs_path = cwd.join(file_path);
                        edit_tool_map
                            .insert(tool_call.call_id.clone(), (abs_path, content.to_string()));
                    }
                }
            }
        }
    }

    // Phase 2: Process tool_results from turn tool_calls
    for turn in history.turns() {
        for tool_call in &turn.tool_calls {
            // Handle Read tool results
            if let Some(path) = read_tool_map.get(&tool_call.call_id)
                && let Some(cocode_protocol::ToolResultContent::Text(content)) = &tool_call.output
            {
                // Strip line number prefixes (cat -n format)
                let clean_content = strip_line_numbers(content);
                let content_hash = FileReadState::compute_hash(&clean_content);
                let timestamp = turn.started_at.timestamp_millis();

                tracker.record_read_with_state(
                    path.clone(),
                    FileReadState {
                        content: Some(clean_content),
                        timestamp: std::time::SystemTime::UNIX_EPOCH
                            + std::time::Duration::from_millis(timestamp as u64),
                        file_mtime: None,
                        content_hash: Some(content_hash),
                        offset: None,
                        limit: None,
                        kind: cocode_protocol::FileReadKind::FullContent,
                        access_count: 1,
                        read_turn: turn.number,
                    },
                );
            }

            // Handle Edit tool results (the new_string is the relevant content)
            if let Some((path, content)) = edit_tool_map.get(&tool_call.call_id)
                && tool_call.status.is_success()
            {
                let timestamp = turn.started_at.timestamp_millis();
                tracker.record_read_with_state(
                    path.clone(),
                    FileReadState {
                        content: Some(content.clone()),
                        timestamp: std::time::SystemTime::UNIX_EPOCH
                            + std::time::Duration::from_millis(timestamp as u64),
                        file_mtime: None,
                        content_hash: Some(FileReadState::compute_hash(content)),
                        offset: None,
                        limit: None,
                        kind: cocode_protocol::FileReadKind::FullContent,
                        access_count: 1,
                        read_turn: turn.number,
                    },
                );
            }
        }
    }

    // Enforce LRU limit: evict oldest entries when count exceeds max_entries
    tracker.enforce_entry_limit(max_entries);

    tracker
}

/// Strip line number prefixes from Read tool output.
///
/// The Read tool outputs content with line numbers in the format "     1\tcontent"
/// (right-aligned 6-char line number, followed by tab, then content).
/// This function removes those prefixes to get the original content.
fn strip_line_numbers(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            // Match pattern: right-aligned line numbers (6 chars) + tab + content
            // Format from Read tool: format!("{line_num:>6}\t{truncated}\n")
            // Examples: "     1\tcontent", "   123\tcontent"
            if let Some(pos) = line.find('\t') {
                let prefix = &line[..pos];
                let trimmed = prefix.trim();
                // Check if everything before the tab is digits
                if trimmed.chars().all(|c| c.is_ascii_digit()) {
                    line[pos + 1..].to_string()
                } else {
                    // Not a line number prefix, keep as-is
                    line.to_string()
                }
            } else {
                // No tab found, keep as-is
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Check if a path is an internal file that should be excluded from restoration.
///
/// Internal files include:
/// - Session memory files (summary.md)
/// - Plan files
/// - Auto memory files (MEMORY.md, etc.)
/// - Tool result persistence files
///
/// # Arguments
/// * `path` - The file path to check
///
/// # Returns
/// `true` if the file should be excluded from restoration.
pub fn is_internal_file(path: &Path, _session_id: &str) -> bool {
    let path_str = path.to_string_lossy();

    // Session memory file
    if path_str.contains("session-memory") && path_str.contains("summary.md") {
        return true;
    }

    // Plan files (in ~/.cocode/plans/)
    if path_str.contains(".cocode/plans/") {
        return true;
    }

    // Auto memory files (MEMORY.md or project memory)
    if let Some(filename) = path.file_name().and_then(|n| n.to_str())
        && (filename == "MEMORY.md" || filename.starts_with("memory-"))
    {
        return true;
    }

    // Tool result persistence files
    if path_str.contains("tool-results/") {
        return true;
    }

    false
}

#[cfg(test)]
#[path = "compaction.test.rs"]
mod tests;
