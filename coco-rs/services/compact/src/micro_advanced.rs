//! Enhanced micro-compaction: budget-aware tool result clearing, file-unchanged
//! stub removal, and thinking block compaction.
//!
//! TS: services/compact/microCompact.ts (530 LOC) — time-based MC, cached MC,
//! tool result clearing with keep-recent semantics.
//!
//! This module extends the basic micro-compaction (`micro.rs`) with:
//! - Token-budget-aware clearing (stop once enough tokens are freed)
//! - Removal of "[file unchanged]" placeholder stubs
//! - Compaction of thinking/reasoning blocks from old turns

use coco_types::Message;

use crate::tokens;
use crate::types::CLEARED_TOOL_RESULT_MESSAGE;
use crate::types::MicrocompactResult;

/// Configuration for budget-aware micro-compaction.
#[derive(Debug, Clone)]
pub struct MicroCompactBudgetConfig {
    /// Maximum tokens to free. Compaction stops once this is met.
    pub tokens_to_free: i64,
    /// Number of recent messages to always keep intact.
    pub keep_recent: usize,
    /// Tools whose results should never be cleared.
    pub exclude_tools: Vec<String>,
}

/// Clear old tool results within a token budget.
///
/// Walks messages oldest-first, replacing tool result content with a
/// placeholder until `tokens_to_free` tokens have been reclaimed. Skips
/// the most recent `keep_recent` messages and any tools in the exclude list.
pub fn micro_compact_with_budget(
    messages: &mut [Message],
    config: &MicroCompactBudgetConfig,
) -> MicrocompactResult {
    let total = messages.len();
    let cutoff = total.saturating_sub(config.keep_recent);

    let mut tokens_freed: i64 = 0;
    let mut cleared: i32 = 0;

    for msg in messages.iter_mut().take(cutoff) {
        if tokens_freed >= config.tokens_to_free {
            break;
        }
        let Message::ToolResult(tr) = msg else {
            continue;
        };

        // Check if this tool is excluded
        if config
            .exclude_tools
            .iter()
            .any(|ex| tr.tool_id.to_string().contains(ex))
        {
            continue;
        }

        let est_tokens = tokens::estimate_tool_result_tokens(tr);
        if est_tokens <= 10 {
            // Already tiny, not worth clearing
            continue;
        }

        // Replace with cleared placeholder
        tr.message = coco_types::LlmMessage::Tool {
            content: vec![coco_types::ToolContent::ToolResult(
                coco_types::ToolResultContent {
                    tool_call_id: tr.tool_use_id.clone(),
                    tool_name: String::new(),
                    output: vercel_ai_provider::ToolResultContent::text(
                        CLEARED_TOOL_RESULT_MESSAGE,
                    ),
                    is_error: false,
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        };

        tokens_freed += est_tokens;
        cleared += 1;
    }

    MicrocompactResult {
        messages_cleared: cleared,
        tokens_saved_estimate: tokens_freed,
        was_time_triggered: false,
    }
}

/// Sentinel text inserted by file-edit tools when no changes were made.
const FILE_UNCHANGED_STUB: &str = "[file unchanged]";

/// Remove "[file unchanged]" placeholder stubs from tool results.
///
/// When file-edit tools detect no changes, they leave a stub. Over many turns
/// these accumulate without value. This function replaces matching tool results
/// with a minimal placeholder, freeing tokens.
pub fn clear_file_unchanged_stubs(messages: &mut [Message]) -> MicrocompactResult {
    let mut cleared: i32 = 0;
    let mut tokens_freed: i64 = 0;

    for msg in messages.iter_mut() {
        let Message::ToolResult(tr) = msg else {
            continue;
        };

        if !tool_result_contains_text(tr, FILE_UNCHANGED_STUB) {
            continue;
        }

        let est_tokens = tokens::estimate_tool_result_tokens(tr);
        tr.message = coco_types::LlmMessage::Tool {
            content: vec![coco_types::ToolContent::ToolResult(
                coco_types::ToolResultContent {
                    tool_call_id: tr.tool_use_id.clone(),
                    tool_name: String::new(),
                    output: vercel_ai_provider::ToolResultContent::text(
                        "[file unchanged - stub cleared]",
                    ),
                    is_error: false,
                    provider_metadata: None,
                },
            )],
            provider_options: None,
        };

        tokens_freed += est_tokens.saturating_sub(8); // placeholder is ~8 tokens
        cleared += 1;
    }

    MicrocompactResult {
        messages_cleared: cleared,
        tokens_saved_estimate: tokens_freed,
        was_time_triggered: false,
    }
}

/// Remove thinking/reasoning blocks from old assistant turns.
///
/// Thinking blocks are valuable during the current reasoning chain but become
/// dead weight in older turns. This function removes them from all assistant
/// messages except the most recent `keep_recent_turns`.
pub fn compact_thinking_blocks(
    messages: &mut [Message],
    keep_recent_turns: usize,
) -> MicrocompactResult {
    // Find assistant message indices
    let assistant_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(i, m)| matches!(m, Message::Assistant(_)).then_some(i))
        .collect();

    let cutoff_count = assistant_indices.len().saturating_sub(keep_recent_turns);
    let indices_to_compact = &assistant_indices[..cutoff_count];

    let mut cleared: i32 = 0;
    let mut tokens_freed: i64 = 0;

    for &idx in indices_to_compact {
        let Message::Assistant(ref mut asst) = messages[idx] else {
            continue;
        };

        let coco_types::LlmMessage::Assistant {
            ref mut content, ..
        } = asst.message
        else {
            continue;
        };

        let original_len = content.len();
        let mut removed_tokens: i64 = 0;

        content.retain(|part| {
            match part {
                coco_types::AssistantContent::Reasoning(r) => {
                    removed_tokens += (r.text.len() as i64) / 4;
                    false // Remove thinking blocks
                }
                _ => true, // Keep everything else
            }
        });

        let removed = (original_len - content.len()) as i32;
        if removed > 0 {
            cleared += removed;
            tokens_freed += removed_tokens;
        }
    }

    MicrocompactResult {
        messages_cleared: cleared,
        tokens_saved_estimate: tokens_freed,
        was_time_triggered: false,
    }
}

/// Check if a tool result message contains specific text (walks content parts).
fn tool_result_contains_text(tr: &coco_types::ToolResultMessage, needle: &str) -> bool {
    let coco_types::LlmMessage::Tool { content, .. } = &tr.message else {
        return false;
    };
    for part in content {
        let coco_types::ToolContent::ToolResult(result) = part else {
            continue;
        };
        match &result.output {
            vercel_ai_provider::ToolResultContent::Text { value, .. } => {
                if value.contains(needle) {
                    return true;
                }
            }
            vercel_ai_provider::ToolResultContent::Content { value, .. } => {
                for sub in value {
                    if let vercel_ai_provider::ToolResultContentPart::Text { text, .. } = sub
                        && text.contains(needle)
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
#[path = "micro_advanced.test.rs"]
mod tests;
