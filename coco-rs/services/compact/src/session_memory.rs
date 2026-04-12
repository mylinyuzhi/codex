//! Session memory compaction: use session memory as a compact summary
//! instead of calling the LLM to re-summarize.
//!
//! TS: services/compact/sessionMemoryCompact.ts (630 LOC)
//!
//! When session memory has been extracted (by the memory extraction pipeline),
//! compaction can use it directly as the summary, avoiding a costly LLM call.
//! This module selects which messages to keep, merges similar memories, and
//! produces a `CompactResult`.

use std::collections::HashMap;

use coco_types::CompactTrigger;
use coco_types::Message;

use crate::tokens;
use crate::types::CompactError;
use crate::types::CompactResult;

/// Configuration for session memory compaction thresholds.
#[derive(Debug, Clone)]
pub struct SessionMemoryCompactConfig {
    /// Minimum tokens to preserve after compaction.
    pub min_tokens: i64,
    /// Minimum number of messages with text blocks to keep.
    pub min_text_block_messages: i32,
    /// Maximum tokens to preserve after compaction (hard cap).
    pub max_tokens: i64,
}

impl Default for SessionMemoryCompactConfig {
    fn default() -> Self {
        Self {
            min_tokens: 10_000,
            min_text_block_messages: 5,
            max_tokens: 40_000,
        }
    }
}

/// Perform session memory compaction: replace old messages with the session
/// memory content as a summary, keeping only recent messages.
///
/// Returns `None` if session memory is empty or unavailable, signaling the
/// caller should fall back to LLM-based compaction.
pub fn compact_session_memory(
    messages: &[Message],
    session_memory: &str,
    config: &SessionMemoryCompactConfig,
) -> Result<Option<CompactResult>, CompactError> {
    let trimmed = session_memory.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let start_index = calculate_messages_to_keep_index(messages, config);
    let adjusted_index = adjust_index_to_preserve_api_invariants(messages, start_index);
    let messages_to_keep: Vec<Message> = messages[adjusted_index..].to_vec();

    // Build the summary from session memory content
    let summary = crate::prompt::get_compact_user_summary_message(
        trimmed, /*suppress_follow_up*/ true, /*transcript_path*/ None,
    );
    let summary_message = Message::User(coco_types::UserMessage {
        message: coco_types::LlmMessage::user_text(&summary),
        uuid: uuid::Uuid::new_v4(),
        timestamp: String::new(),
        is_meta: false,
        is_visible_in_transcript_only: true,
        is_virtual: false,
        is_compact_summary: true,
        permission_mode: None,
        origin: None,
    });

    let pre_tokens = tokens::estimate_tokens_conservative(messages);
    let post_tokens = tokens::estimate_tokens_conservative(&messages_to_keep)
        + tokens::estimate_text_tokens(&summary);

    let messages_summarized = (messages.len() - messages_to_keep.len()) as i32;
    let boundary = Message::System(coco_types::SystemMessage::CompactBoundary(
        coco_types::SystemCompactBoundaryMessage {
            uuid: uuid::Uuid::new_v4(),
            tokens_before: pre_tokens,
            tokens_after: post_tokens,
            trigger: CompactTrigger::Auto,
            user_context: None,
            messages_summarized: Some(messages_summarized),
            pre_compact_discovered_tools: vec![],
            preserved_segment: None,
        },
    ));

    Ok(Some(CompactResult {
        boundary_marker: boundary,
        summary_messages: vec![summary_message],
        attachments: vec![],
        messages_to_keep,
        hook_results: vec![],
        user_display_message: None,
        pre_compact_tokens: pre_tokens,
        post_compact_tokens: post_tokens,
        true_post_compact_tokens: post_tokens,
        is_recompaction: false,
        trigger: CompactTrigger::Auto,
    }))
}

/// Select which memories to compact when the memory directory grows too large.
///
/// Picks the oldest/least-recently-referenced entries first, based on
/// `last_used` timestamps. Returns entry names sorted by compaction priority
/// (first = most eligible for removal/merging).
pub fn select_memories_for_compaction(
    entries: &[(String, MemoryMetadata)],
    max_to_keep: usize,
) -> Vec<String> {
    if entries.len() <= max_to_keep {
        return Vec::new();
    }

    let mut scored: Vec<(f64, &str)> = entries
        .iter()
        .map(|(name, meta)| {
            // Score: lower = more eligible for compaction.
            // Staleness (days since last use) dominates, with a small weight
            // for access count so frequently-used old entries survive longer.
            let staleness_days = meta.staleness_days as f64;
            let frequency_bonus = (meta.access_count as f64).ln().max(0.0) * 5.0;
            let score = staleness_days - frequency_bonus;
            (score, name.as_str())
        })
        .collect();

    // Sort descending by score (highest staleness first)
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let to_remove = entries.len() - max_to_keep;
    scored
        .into_iter()
        .take(to_remove)
        .map(|(_, name)| name.to_string())
        .collect()
}

/// Metadata used for compaction scoring.
#[derive(Debug, Clone)]
pub struct MemoryMetadata {
    /// Days since this memory was last accessed/referenced.
    pub staleness_days: i32,
    /// Number of times this memory has been accessed.
    pub access_count: i32,
}

/// Merge similar/overlapping memories into consolidated entries.
///
/// Groups memories by their name prefix (the part before the last `_` or `-`)
/// and merges entries within each group by concatenating their content with
/// deduplication of identical lines.
pub fn merge_similar_memories(memories: &[(String, String)]) -> Vec<(String, String)> {
    // Group by name prefix
    let mut groups: HashMap<String, Vec<(&str, &str)>> = HashMap::new();
    for (name, content) in memories {
        let prefix = extract_name_prefix(name);
        groups
            .entry(prefix.to_string())
            .or_default()
            .push((name, content));
    }

    let mut merged = Vec::new();
    for (prefix, group) in &groups {
        if group.len() <= 1 {
            for (name, content) in group {
                merged.push(((*name).to_string(), (*content).to_string()));
            }
            continue;
        }

        // Merge: use the prefix as the canonical name, deduplicate lines
        let mut seen_lines = std::collections::HashSet::new();
        let mut merged_content = String::new();
        for (_, content) in group {
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && seen_lines.insert(trimmed.to_string()) {
                    if !merged_content.is_empty() {
                        merged_content.push('\n');
                    }
                    merged_content.push_str(line);
                }
            }
        }
        merged.push((prefix.clone(), merged_content));
    }

    merged.sort_by(|a, b| a.0.cmp(&b.0));
    merged
}

// ── Internal helpers ────────────────────────────────────────────────

/// Calculate the starting index for messages to keep after compaction.
///
/// Starts from the end and expands backwards until we meet both minimum
/// thresholds (tokens and text-block messages), or hit the max cap.
fn calculate_messages_to_keep_index(
    messages: &[Message],
    config: &SessionMemoryCompactConfig,
) -> usize {
    if messages.is_empty() {
        return 0;
    }

    let mut start_index = messages.len();
    let mut total_tokens: i64 = 0;
    let mut text_block_count: i32 = 0;

    for i in (0..messages.len()).rev() {
        let msg_tokens = tokens::estimate_message_tokens(&messages[i]);
        let would_be = total_tokens + msg_tokens;

        // Stop if adding this message would exceed the max cap
        if would_be > config.max_tokens && total_tokens > 0 {
            break;
        }

        total_tokens = would_be;
        if has_text_blocks(&messages[i]) {
            text_block_count += 1;
        }
        start_index = i;

        // Once we meet both minimums, stop expanding
        if total_tokens >= config.min_tokens && text_block_count >= config.min_text_block_messages {
            break;
        }
    }

    start_index
}

/// Adjust the keep index to preserve tool_use/tool_result pairs.
///
/// TS: `adjustIndexToPreserveAPIInvariants()` — if the start index lands
/// on a tool_result, we need to include the preceding assistant message
/// (which contains the tool_use) to maintain API invariants.
fn adjust_index_to_preserve_api_invariants(messages: &[Message], start_index: usize) -> usize {
    if start_index == 0 || start_index >= messages.len() {
        return start_index;
    }

    let mut idx = start_index;

    // Walk backwards past any tool_result messages to find the owning assistant
    while idx > 0 && matches!(messages[idx], Message::ToolResult(_)) {
        idx -= 1;
    }

    // If we landed on an assistant message, include it (it has the tool_use).
    // Also include the preceding user message if present to keep the turn intact.
    if matches!(messages[idx], Message::Assistant(_))
        && idx > 0
        && matches!(messages[idx - 1], Message::User(_))
    {
        idx -= 1;
    }

    idx
}

/// Check if a message contains meaningful text content.
/// TS: `hasTextBlocks(message)` in sessionMemoryCompact.ts.
pub fn has_text_blocks(message: &Message) -> bool {
    match message {
        Message::User(u) => match &u.message {
            coco_types::LlmMessage::User { content, .. } => content
                .iter()
                .any(|c| matches!(c, coco_types::UserContent::Text(_))),
            _ => false,
        },
        Message::Assistant(a) => match &a.message {
            coco_types::LlmMessage::Assistant { content, .. } => content
                .iter()
                .any(|c| matches!(c, coco_types::AssistantContent::Text(_))),
            _ => false,
        },
        _ => false,
    }
}

/// Extract the prefix from a memory name (before the last `_` or `-`).
fn extract_name_prefix(name: &str) -> &str {
    let base = name.strip_suffix(".md").unwrap_or(name);
    if let Some(pos) = base.rfind(['_', '-']) {
        &base[..pos]
    } else {
        base
    }
}

#[cfg(test)]
#[path = "session_memory.test.rs"]
mod tests;
