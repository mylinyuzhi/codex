//! Basic micro-compaction: clear old tool results to free context.
//!
//! TS: microCompact.ts — replaces old tool result content with a placeholder.
//! Preserves message structure (tool_use/tool_result pairing) — does NOT
//! tombstone messages.

use coco_types::Message;
use coco_types::ToolId;
use coco_types::ToolName;

use crate::tokens;
use crate::types::CLEARED_TOOL_RESULT_MESSAGE;
use crate::types::MicrocompactResult;

/// Tools whose results can be cleared during micro-compaction.
/// Matches TS `COMPACTABLE_TOOLS` set in microCompact.ts.
/// TS: `COMPACTABLE_TOOLS` in microCompact.ts.
/// Exactly: FileRead, ...SHELL_TOOL_NAMES, Grep, Glob, WebSearch, WebFetch, FileEdit, FileWrite.
const COMPACTABLE_TOOLS: &[ToolName] = &[
    ToolName::Read,
    ToolName::Bash,
    ToolName::PowerShell,
    ToolName::Grep,
    ToolName::Glob,
    ToolName::WebSearch,
    ToolName::WebFetch,
    ToolName::Edit,
    ToolName::Write,
];

/// Perform micro-compaction: clear old tool results to free context.
///
/// Replaces tool result content with `[Old tool result content cleared]`
/// while preserving the message structure required by the API. Only clears
/// results from COMPACTABLE_TOOLS. Skips the most recent `keep_recent`
/// messages.
pub fn micro_compact(messages: &mut [Message], keep_recent: usize) -> MicrocompactResult {
    let total = messages.len();
    let cutoff = total.saturating_sub(keep_recent);
    let mut cleared: i32 = 0;
    let mut tokens_freed: i64 = 0;

    for msg in messages.iter_mut().take(cutoff) {
        let Message::ToolResult(tr) = msg else {
            continue;
        };

        if !is_compactable_tool(&tr.tool_id) {
            continue;
        }

        // Skip if already cleared
        if is_already_cleared(tr) {
            continue;
        }

        let est_tokens = tokens::estimate_tool_result_tokens(tr);
        if est_tokens <= 10 {
            continue;
        }

        // Replace content with cleared placeholder (preserves message structure)
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

/// Check if a tool is in the compactable set.
fn is_compactable_tool(tool_id: &ToolId) -> bool {
    match tool_id {
        ToolId::Builtin(name) => COMPACTABLE_TOOLS.contains(name),
        // MCP and custom tools: always compactable (external tools often produce large output)
        ToolId::Mcp { .. } | ToolId::Custom(_) => true,
    }
}

/// Check if a tool result has already been cleared.
fn is_already_cleared(tr: &coco_types::ToolResultMessage) -> bool {
    if let coco_types::LlmMessage::Tool { content, .. } = &tr.message
        && content.len() == 1
        && let coco_types::ToolContent::ToolResult(part) = &content[0]
        && let vercel_ai_provider::ToolResultContent::Text { value, .. } = &part.output
    {
        return value == CLEARED_TOOL_RESULT_MESSAGE;
    }
    false
}
