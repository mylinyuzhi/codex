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

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;
use tracing::{debug, info};

// Re-export commonly used types and constants from protocol for convenience
pub use cocode_protocol::CompactConfig;

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

fn default_session_memory_min_savings() -> i32 {
    10_000
}

impl Default for SessionMemoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_session_memory_enabled(),
            summary_path: None,
            min_savings_tokens: default_session_memory_min_savings(),
            last_summarized_id: None,
        }
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
            if name == "TodoWrite" {
                if let Some(todos) = input.get("todos").and_then(|t| t.as_array()) {
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

                            let owner =
                                todo.get("owner").and_then(|v| v.as_str()).map(String::from);

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
        }

        Self::default()
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
    /// File paths that were persisted.
    pub persisted_files: Vec<PathBuf>,
    /// File paths from Read tool results that were compacted.
    ///
    /// The caller can use this to clean up file state tracking (readFileState).
    pub cleared_file_paths: Vec<PathBuf>,
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

    // Phase 4: Memory attachment cleanup (placeholder - would clear memory UUIDs)
    let cleared_memory_uuids = Vec::new();

    // Phase 5: Content replacement
    let mut result = MicroCompactResult::default();
    for candidate in candidates_to_compact {
        let msg = &mut messages[candidate.index as usize];

        // Get original content for potential persistence
        let original_content = msg
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

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

        // Persist large results if directory provided
        if let Some(dir) = persist_dir {
            if let Some(ref tool_use_id) = candidate.tool_use_id {
                let file_path = dir.join(format!("tool-results/{tool_use_id}.txt"));
                if let Some(parent) = file_path.parent() {
                    if std::fs::create_dir_all(parent).is_ok() {
                        if std::fs::write(&file_path, &original_content).is_ok() {
                            result.persisted_files.push(file_path);
                        }
                    }
                }
            }
        }

        // Generate preview + replacement marker
        let preview = if original_content.len() > CONTENT_PREVIEW_LENGTH {
            format!(
                "{}...\n\n{}",
                &original_content[..CONTENT_PREVIEW_LENGTH],
                CLEARED_CONTENT_MARKER
            )
        } else {
            CLEARED_CONTENT_MARKER.to_string()
        };

        // Replace content
        if let Some(content) = msg.get_mut("content") {
            *content = serde_json::Value::String(preview);
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
            .map_or(0, |s| s.len());
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
    if let Some(task_status) = tasks {
        if !task_status.tasks.is_empty() {
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
            .map_or(0, |s| s.len());

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
    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
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
        "---\nlast_summarized_id: {}\ntimestamp: {}\n---\n{}",
        last_summarized_id, timestamp, summary
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
    let mut result = ContextRestoration::default();
    let mut remaining = budget;

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

    // Priority 4: Files (sorted by priority, limited by max files)
    let mut sorted_files = files;
    sorted_files.sort_by(|a, b| b.priority.cmp(&a.priority));

    for file in sorted_files
        .into_iter()
        .take(CONTEXT_RESTORATION_MAX_FILES as usize)
    {
        if file.tokens <= remaining {
            remaining -= file.tokens;
            result.files.push(file);
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_compaction_config() {
        let config = CompactionConfig::default();
        assert!((config.threshold - 0.8).abs() < f64::EPSILON);
        assert!(config.micro_compact);
        assert_eq!(config.min_messages_to_keep, 4);
    }

    #[test]
    fn test_should_compact_below_threshold() {
        assert!(!should_compact(7000, 10000, 0.8));
    }

    #[test]
    fn test_should_compact_at_threshold() {
        assert!(should_compact(8000, 10000, 0.8));
    }

    #[test]
    fn test_should_compact_above_threshold() {
        assert!(should_compact(9500, 10000, 0.8));
    }

    #[test]
    fn test_should_compact_zero_max() {
        assert!(!should_compact(100, 0, 0.8));
    }

    #[test]
    fn test_should_compact_negative_max() {
        assert!(!should_compact(100, -1, 0.8));
    }

    #[test]
    fn test_micro_compact_candidates_empty() {
        let messages: Vec<serde_json::Value> = vec![];
        assert!(micro_compact_candidates(&messages).is_empty());
    }

    #[test]
    fn test_micro_compact_candidates_no_tool_results() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "assistant", "content": "hi"}),
        ];
        assert!(micro_compact_candidates(&messages).is_empty());
    }

    #[test]
    fn test_micro_compact_candidates_small_tool_result() {
        let messages = vec![serde_json::json!({"role": "tool", "content": "ok"})];
        assert!(micro_compact_candidates(&messages).is_empty());
    }

    #[test]
    fn test_micro_compact_candidates_large_tool_result() {
        let large_content = "x".repeat(3000);
        let messages = vec![
            serde_json::json!({"role": "user", "content": "do something"}),
            serde_json::json!({"role": "tool", "content": large_content}),
            serde_json::json!({"role": "assistant", "content": "done"}),
        ];
        let candidates = micro_compact_candidates(&messages);
        assert_eq!(candidates, vec![1]);
    }

    #[test]
    fn test_micro_compact_candidates_tool_result_role() {
        let large_content = "y".repeat(2500);
        let messages = vec![serde_json::json!({"role": "tool_result", "content": large_content})];
        let candidates = micro_compact_candidates(&messages);
        assert_eq!(candidates, vec![0]);
    }

    #[test]
    fn test_parse_session_memory_simple() {
        let content = "This is a summary of the conversation.";
        let summary = parse_session_memory(content).unwrap();
        assert_eq!(summary.summary, "This is a summary of the conversation.");
        assert!(summary.last_summarized_id.is_none());
    }

    #[test]
    fn test_parse_session_memory_with_frontmatter() {
        let content = "---\nlast_summarized_id: turn-42\n---\nSummary content here.";
        let summary = parse_session_memory(content).unwrap();
        assert_eq!(summary.summary, "Summary content here.");
        assert_eq!(summary.last_summarized_id, Some("turn-42".to_string()));
    }

    #[test]
    fn test_parse_session_memory_empty() {
        let content = "";
        assert!(parse_session_memory(content).is_none());
    }

    #[test]
    fn test_build_context_restoration_within_budget() {
        let files = vec![
            FileRestoration {
                path: PathBuf::from("/test/file1.rs"),
                content: "fn main() {}".to_string(),
                priority: 10,
                tokens: 100,
            },
            FileRestoration {
                path: PathBuf::from("/test/file2.rs"),
                content: "struct Foo {}".to_string(),
                priority: 5,
                tokens: 50,
            },
        ];

        let restoration =
            build_context_restoration(files, Some("- TODO 1".to_string()), None, vec![], 500);

        assert!(restoration.todos.is_some());
        assert_eq!(restoration.files.len(), 2);
        // Higher priority file should be first
        assert_eq!(restoration.files[0].path, PathBuf::from("/test/file1.rs"));
    }

    #[test]
    fn test_build_context_restoration_budget_exceeded() {
        let files = vec![FileRestoration {
            path: PathBuf::from("/test/large.rs"),
            content: "x".repeat(10000),
            priority: 10,
            tokens: 2500,
        }];

        // Budget too small for the file
        let restoration = build_context_restoration(files, None, None, vec![], 100);
        assert!(restoration.files.is_empty());
    }

    #[test]
    fn test_format_restoration_message_empty() {
        let restoration = ContextRestoration::default();
        let msg = format_restoration_message(&restoration);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_format_restoration_message_with_content() {
        let mut restoration = ContextRestoration::default();
        restoration.todos = Some("- Fix bug".to_string());
        restoration.files.push(FileRestoration {
            path: PathBuf::from("/test.rs"),
            content: "fn main() {}".to_string(),
            priority: 1,
            tokens: 10,
        });

        let msg = format_restoration_message(&restoration);
        assert!(msg.contains("<restored_context>"));
        assert!(msg.contains("<todo_list>"));
        assert!(msg.contains("- Fix bug"));
        assert!(msg.contains("<file path=\"/test.rs\">"));
    }

    #[test]
    fn test_session_memory_config_default() {
        let config = SessionMemoryConfig::default();
        assert!(!config.enabled);
        assert!(config.summary_path.is_none());
        assert_eq!(config.min_savings_tokens, 10_000);
    }

    #[test]
    fn test_compaction_tier_variants() {
        let tiers = vec![
            CompactionTier::SessionMemory,
            CompactionTier::Full,
            CompactionTier::Micro,
        ];
        for tier in tiers {
            let json = serde_json::to_string(&tier).unwrap();
            let back: CompactionTier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, back);
        }
    }

    // ========================================================================
    // Phase 2: Threshold Status Tests
    // ========================================================================

    #[test]
    fn test_threshold_status_ok() {
        let config = CompactConfig::default();
        // Well below any threshold
        let status = ThresholdStatus::calculate(50000, 200000, &config);

        assert!(status.percent_left > 0.7);
        assert!(!status.is_above_warning_threshold);
        assert!(!status.is_above_error_threshold);
        assert!(!status.is_above_auto_compact_threshold);
        assert!(!status.is_at_blocking_limit);
        assert_eq!(status.status_description(), "ok");
        assert!(!status.needs_action());
    }

    #[test]
    fn test_threshold_status_warning() {
        let config = CompactConfig::default();
        // Above warning but below auto-compact
        // target = 200000 - 13000 = 187000
        // warning = 187000 - 20000 = 167000
        let status = ThresholdStatus::calculate(170000, 200000, &config);

        assert!(status.is_above_warning_threshold);
        assert!(status.needs_action());
    }

    #[test]
    fn test_threshold_status_auto_compact() {
        let config = CompactConfig::default();
        // Above auto-compact threshold
        // target = 200000 - 13000 = 187000
        let status = ThresholdStatus::calculate(190000, 200000, &config);

        assert!(status.is_above_warning_threshold);
        assert!(status.is_above_error_threshold);
        assert!(status.is_above_auto_compact_threshold);
        assert_eq!(status.status_description(), "auto-compact");
    }

    #[test]
    fn test_threshold_status_blocking() {
        let config = CompactConfig::default();
        // At blocking limit
        // blocking = 200000 - 3000 = 197000
        let status = ThresholdStatus::calculate(198000, 200000, &config);

        assert!(status.is_at_blocking_limit);
        assert_eq!(status.status_description(), "blocking");
    }

    #[test]
    fn test_threshold_status_zero_available() {
        let config = CompactConfig::default();
        let status = ThresholdStatus::calculate(100, 0, &config);

        assert!(status.is_at_blocking_limit);
        assert_eq!(status.percent_left, 0.0);
    }

    // ========================================================================
    // Phase 2: Compactable Tools Tests
    // ========================================================================

    #[test]
    fn test_compactable_tools_set() {
        assert!(COMPACTABLE_TOOLS.contains("Read"));
        assert!(COMPACTABLE_TOOLS.contains("Bash"));
        assert!(COMPACTABLE_TOOLS.contains("Grep"));
        assert!(COMPACTABLE_TOOLS.contains("Glob"));
        assert!(COMPACTABLE_TOOLS.contains("WebSearch"));
        assert!(COMPACTABLE_TOOLS.contains("WebFetch"));
        assert!(COMPACTABLE_TOOLS.contains("Edit"));
        assert!(COMPACTABLE_TOOLS.contains("Write"));

        // Non-compactable tools
        assert!(!COMPACTABLE_TOOLS.contains("Task"));
        assert!(!COMPACTABLE_TOOLS.contains("AskUser"));
    }

    // ========================================================================
    // Phase 2: Micro-Compact Execution Tests
    // ========================================================================

    #[test]
    fn test_collect_tool_result_candidates() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-1",
                "content": "file content here"
            }),
            serde_json::json!({"role": "assistant", "content": "done"}),
            serde_json::json!({
                "role": "tool_result",
                "name": "Bash",
                "tool_use_id": "tool-2",
                "content": "command output"
            }),
        ];

        let candidates = collect_tool_result_candidates(&messages);
        assert_eq!(candidates.len(), 2);

        assert_eq!(candidates[0].index, 1);
        assert_eq!(candidates[0].tool_name, Some("Read".to_string()));
        assert!(candidates[0].is_compactable);

        assert_eq!(candidates[1].index, 3);
        assert_eq!(candidates[1].tool_name, Some("Bash".to_string()));
        assert!(candidates[1].is_compactable);
    }

    #[test]
    fn test_execute_micro_compact_disabled() {
        let mut messages = vec![serde_json::json!({"role": "user", "content": "test"})];
        let mut config = CompactConfig::default();
        config.disable_micro_compact = true;

        let result = execute_micro_compact(&mut messages, 100000, 200000, &config, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_execute_micro_compact_no_candidates() {
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "assistant", "content": "hi"}),
        ];
        let config = CompactConfig::default();

        let result = execute_micro_compact(&mut messages, 100000, 200000, &config, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_execute_micro_compact_below_threshold() {
        let large_content = "x".repeat(5000);
        let mut messages = vec![
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-1",
                "content": large_content
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-2",
                "content": large_content
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-3",
                "content": large_content
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-4",
                "content": large_content
            }),
        ];
        let config = CompactConfig::default();

        // Context usage well below warning threshold
        let result = execute_micro_compact(&mut messages, 50000, 200000, &config, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_execute_micro_compact_success() {
        // Large content: 50000 chars = ~12500 tokens each
        // With 5 candidates and keeping 3, we compact 2
        // Potential savings: 2 * 12500 = 25000 tokens > 20000 min savings
        let large_content = "x".repeat(50000);
        let mut messages = vec![
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-1",
                "content": large_content.clone()
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-2",
                "content": large_content.clone()
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-3",
                "content": large_content.clone()
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-4",
                "content": large_content.clone()
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-5",
                "content": large_content
            }),
        ];
        let config = CompactConfig::default();

        // Context usage above warning threshold (167000 for 200K available)
        let result = execute_micro_compact(&mut messages, 180000, 200000, &config, None);

        assert!(result.is_some());
        let result = result.unwrap();
        // Should compact 2 results (5 - 3 recent to keep)
        assert_eq!(result.compacted_count, 2);
        assert!(result.tokens_saved > 0);

        // First two messages should have been compacted
        let content1 = messages[0]["content"].as_str().unwrap();
        assert!(content1.contains(CLEARED_CONTENT_MARKER));

        let content2 = messages[1]["content"].as_str().unwrap();
        assert!(content2.contains(CLEARED_CONTENT_MARKER));

        // Last three should be unchanged
        let content5 = messages[4]["content"].as_str().unwrap();
        assert!(!content5.contains(CLEARED_CONTENT_MARKER));
    }

    #[test]
    fn test_execute_micro_compact_tracks_file_paths() {
        // Test that micro-compact tracks file paths from Read tool results
        let large_content = "x".repeat(50000);
        let mut messages = vec![
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-1",
                "file_path": "/src/main.rs",
                "content": large_content.clone()
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-2",
                "input": {"file_path": "/src/lib.rs"},
                "content": large_content.clone()
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Bash",
                "tool_use_id": "tool-3",
                "content": large_content.clone()
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-4",
                "file_path": "/src/test.rs",
                "content": large_content.clone()
            }),
            serde_json::json!({
                "role": "tool",
                "name": "Read",
                "tool_use_id": "tool-5",
                "file_path": "/src/config.rs",
                "content": large_content
            }),
        ];
        let config = CompactConfig::default();

        // Context usage above warning threshold
        let result = execute_micro_compact(&mut messages, 180000, 200000, &config, None);

        assert!(result.is_some());
        let result = result.unwrap();

        // Should compact 2 results (5 - 3 recent to keep)
        assert_eq!(result.compacted_count, 2);

        // Should track file paths from compacted Read tool results
        // First two are compacted: tool-1 (Read) and tool-2 (Read)
        // tool-3 (Bash) was before tool-4 and tool-5 which are kept
        assert_eq!(result.cleared_file_paths.len(), 2);
        assert!(
            result
                .cleared_file_paths
                .contains(&PathBuf::from("/src/main.rs"))
        );
        assert!(
            result
                .cleared_file_paths
                .contains(&PathBuf::from("/src/lib.rs"))
        );
    }

    // ========================================================================
    // Phase 2: Compact Instructions Tests
    // ========================================================================

    #[test]
    fn test_build_compact_instructions() {
        let instructions = build_compact_instructions(16000);

        // Check all 9 sections are present
        assert!(instructions.contains("1. Summary Purpose and Scope"));
        assert!(instructions.contains("2. Key Decisions and Outcomes"));
        assert!(instructions.contains("3. Code Changes Made"));
        assert!(instructions.contains("4. Files Modified"));
        assert!(instructions.contains("5. Errors Encountered and Resolutions"));
        assert!(instructions.contains("6. User Preferences Learned"));
        assert!(instructions.contains("7. Pending Tasks and Next Steps"));
        assert!(instructions.contains("8. Important Context to Preserve"));
        assert!(instructions.contains("9. Format"));

        // Check max tokens is included
        assert!(instructions.contains("16000"));
    }

    // ========================================================================
    // Phase 2: Task Status Restoration Tests
    // ========================================================================

    #[test]
    fn test_format_restoration_with_tasks() {
        let mut restoration = ContextRestoration::default();
        restoration.todos = Some("- Fix bug".to_string());

        let tasks = TaskStatusRestoration {
            tasks: vec![
                TaskInfo {
                    id: "task-1".to_string(),
                    subject: "Implement feature".to_string(),
                    status: "in_progress".to_string(),
                    owner: Some("agent-1".to_string()),
                },
                TaskInfo {
                    id: "task-2".to_string(),
                    subject: "Write tests".to_string(),
                    status: "pending".to_string(),
                    owner: None,
                },
            ],
        };

        let msg = format_restoration_with_tasks(&restoration, Some(&tasks));

        assert!(msg.contains("<restored_context>"));
        assert!(msg.contains("<todo_list>"));
        assert!(msg.contains("<task_status>"));
        assert!(msg.contains("[in_progress] task-1"));
        assert!(msg.contains("(agent-1)"));
        assert!(msg.contains("[pending] task-2"));
        assert!(msg.contains("(unassigned)"));
    }

    #[test]
    fn test_format_restoration_with_empty_tasks() {
        let restoration = ContextRestoration::default();
        let tasks = TaskStatusRestoration { tasks: vec![] };

        let msg = format_restoration_with_tasks(&restoration, Some(&tasks));
        assert!(msg.is_empty());
    }

    #[test]
    fn test_task_info_serde() {
        let task = TaskInfo {
            id: "task-1".to_string(),
            subject: "Test task".to_string(),
            status: "pending".to_string(),
            owner: Some("agent".to_string()),
        };

        let json = serde_json::to_string(&task).unwrap();
        let parsed: TaskInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, "task-1");
        assert_eq!(parsed.subject, "Test task");
        assert_eq!(parsed.status, "pending");
        assert_eq!(parsed.owner, Some("agent".to_string()));
    }

    #[test]
    fn test_task_status_from_tool_calls() {
        let tool_calls = vec![
            (
                "Read".to_string(),
                serde_json::json!({"path": "/tmp/file.txt"}),
            ),
            (
                "TodoWrite".to_string(),
                serde_json::json!({
                    "todos": [
                        {"id": "1", "subject": "Fix bug", "status": "completed"},
                        {"id": "2", "subject": "Add tests", "status": "in_progress"},
                        {"id": "3", "subject": "Deploy", "status": "pending"}
                    ]
                }),
            ),
        ];

        let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
        assert_eq!(task_status.tasks.len(), 3);
        assert_eq!(task_status.tasks[0].id, "1");
        assert_eq!(task_status.tasks[0].subject, "Fix bug");
        assert_eq!(task_status.tasks[0].status, "completed");
        assert_eq!(task_status.tasks[1].status, "in_progress");
        assert_eq!(task_status.tasks[2].status, "pending");
    }

    #[test]
    fn test_task_status_from_tool_calls_empty() {
        let tool_calls: Vec<(String, serde_json::Value)> = vec![];
        let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
        assert!(task_status.tasks.is_empty());
    }

    #[test]
    fn test_task_status_from_tool_calls_uses_latest() {
        let tool_calls = vec![
            (
                "TodoWrite".to_string(),
                serde_json::json!({
                    "todos": [
                        {"id": "old", "subject": "Old task", "status": "pending"}
                    ]
                }),
            ),
            (
                "TodoWrite".to_string(),
                serde_json::json!({
                    "todos": [
                        {"id": "new", "subject": "New task", "status": "in_progress"}
                    ]
                }),
            ),
        ];

        let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
        assert_eq!(task_status.tasks.len(), 1);
        // Should use the most recent (last) TodoWrite call
        assert_eq!(task_status.tasks[0].id, "new");
        assert_eq!(task_status.tasks[0].subject, "New task");
    }

    #[test]
    fn test_task_status_from_tool_calls_with_legacy_content() {
        let tool_calls = vec![(
            "TodoWrite".to_string(),
            serde_json::json!({
                "todos": [
                    {"id": "1", "content": "Legacy task description", "status": "pending"}
                ]
            }),
        )];

        let task_status = TaskStatusRestoration::from_tool_calls(&tool_calls);
        assert_eq!(task_status.tasks.len(), 1);
        assert_eq!(task_status.tasks[0].subject, "Legacy task description");
    }

    // ========================================================================
    // Phase 3: Session Memory Write Tests
    // ========================================================================

    #[tokio::test]
    async fn test_write_session_memory() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join(format!(
            "cocode-test-session-memory-{}.md",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let summary = "## Summary\nThis is a test summary.";
        let turn_id = "turn-42";

        // Write session memory
        let result = write_session_memory(&test_path, summary, turn_id).await;
        assert!(result.is_ok());

        // Read and verify
        let content = std::fs::read_to_string(&test_path).unwrap();

        // Check frontmatter
        assert!(content.starts_with("---\n"));
        assert!(content.contains("last_summarized_id: turn-42"));
        assert!(content.contains("timestamp:"));
        assert!(content.contains("---\n## Summary\nThis is a test summary."));

        // Parse it back
        let parsed = parse_session_memory(&content).unwrap();
        assert_eq!(parsed.last_summarized_id, Some("turn-42".to_string()));
        assert!(parsed.summary.contains("## Summary"));

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }

    #[tokio::test]
    async fn test_write_session_memory_creates_parent_dirs() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join(format!(
            "cocode-test-deep/{}/summary.md",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let summary = "Test with nested dirs";
        let turn_id = "turn-1";

        // Write should create parent directories
        let result = write_session_memory(&test_path, summary, turn_id).await;
        assert!(result.is_ok());

        // Verify file exists
        assert!(test_path.exists());

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
        let _ = std::fs::remove_dir(test_path.parent().unwrap());
    }

    #[test]
    fn test_try_session_memory_compact_disabled() {
        let config = SessionMemoryConfig {
            enabled: false,
            ..Default::default()
        };

        let result = try_session_memory_compact(&config);
        assert!(result.is_none());
    }

    #[test]
    fn test_try_session_memory_compact_no_path() {
        let config = SessionMemoryConfig {
            enabled: true,
            summary_path: None,
            ..Default::default()
        };

        let result = try_session_memory_compact(&config);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_session_memory_roundtrip() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join(format!(
            "cocode-test-roundtrip-{}.md",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        // Write
        let original_summary = "## Code Changes\n- Added new feature\n- Fixed bug in auth";
        let turn_id = "turn-99";
        write_session_memory(&test_path, original_summary, turn_id)
            .await
            .unwrap();

        // Read via try_session_memory_compact
        let config = SessionMemoryConfig {
            enabled: true,
            summary_path: Some(test_path.clone()),
            ..Default::default()
        };

        let result = try_session_memory_compact(&config);
        assert!(result.is_some());

        let summary = result.unwrap();
        assert_eq!(summary.last_summarized_id, Some("turn-99".to_string()));
        assert!(summary.summary.contains("## Code Changes"));
        assert!(summary.summary.contains("Added new feature"));
        assert!(summary.token_estimate > 0);

        // Cleanup
        let _ = std::fs::remove_file(&test_path);
    }
}
