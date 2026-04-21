//! Post-compact file restoration.
//!
//! TS: `createPostCompactFileAttachments()` in compact.ts — after compaction,
//! re-injects the N most recently read files so the model retains context for
//! files it was actively working on.
//!
//! Flow:
//! 1. Caller snapshots `FileReadState` before clearing it.
//! 2. Caller passes the snapshot here.
//! 3. We filter out files already visible in preserved messages, plan files,
//!    and CLAUDE.md/memory files.
//! 4. We re-read from disk via `generate_file_attachment` and produce
//!    `AttachmentMessage`s within a token budget.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use coco_context::attachment::FileReadOptions;
use coco_context::attachment::generate_file_attachment;
use coco_context::file_read_state::FileReadEntry;
use coco_types::AttachmentMessage;
use coco_types::LlmMessage;
use coco_types::Message;
use coco_types::ToolName;
use vercel_ai_provider::AssistantContentPart;

use crate::tokens;
use crate::types::POST_COMPACT_MAX_FILES_TO_RESTORE;
use crate::types::POST_COMPACT_MAX_TOKENS_PER_FILE;
use crate::types::POST_COMPACT_TOKEN_BUDGET;

/// Sentinel text prefix for file-unchanged stubs in tool results.
///
/// TS: `FILE_UNCHANGED_STUB` — tool results starting with this are dedup stubs,
/// not actual file reads. Their corresponding tool_use should be excluded from
/// the "already read in preserved messages" set.
const FILE_UNCHANGED_STUB_PREFIX: &str = "File unchanged since last read";

/// Create post-compact file attachment messages.
///
/// TS: `createPostCompactFileAttachments()` — re-injects the most recently
/// accessed files from the pre-compact `FileReadState` snapshot, skipping
/// files already visible in preserved messages and excluded paths.
///
/// `snapshot` is ordered by LRU recency (most recent last, from
/// `FileReadState::snapshot_by_recency`).
pub fn create_post_compact_file_attachments(
    snapshot: &[(PathBuf, FileReadEntry)],
    preserved_messages: &[Message],
    cwd: &Path,
    plan_file: Option<&Path>,
) -> Vec<AttachmentMessage> {
    let preserved_read_paths = collect_read_tool_file_paths(preserved_messages);

    // Filter: skip excluded paths and files already in preserved messages.
    // Reverse so most-recently-accessed comes first.
    let candidates: Vec<&(PathBuf, FileReadEntry)> = snapshot
        .iter()
        .rev()
        .filter(|(path, _)| {
            !should_exclude_from_restore(path, cwd, plan_file)
                && !preserved_read_paths.contains(path)
        })
        .take(POST_COMPACT_MAX_FILES_TO_RESTORE)
        .collect();

    let read_options = FileReadOptions {
        max_tokens: Some(POST_COMPACT_MAX_TOKENS_PER_FILE),
        ..Default::default()
    };

    let read_tool_name = ToolName::Read.as_str();
    let mut used_tokens: i64 = 0;
    let mut result = Vec::new();

    for (path, _entry) in &candidates {
        let Some(att) = generate_file_attachment(path, cwd, &read_options) else {
            continue;
        };

        // Only restore text file attachments (skip images, PDFs, etc.)
        let coco_context::attachment::Attachment::File(ref f) = att else {
            continue;
        };

        // Format as system-reminder message matching the Read tool pattern.
        // TS: `createAttachmentMessage(attachment)` wraps as system-reminder.
        let text = format!(
            "Called the {read_tool_name} tool with the following input: \
             {{\"file_path\":\"{filename}\"}}\n\
             Result of calling the {read_tool_name} tool:\n{content}",
            filename = f.filename,
            content = f.content,
        );

        // TS: roughTokenCountEstimation(jsonStringify(result)) — estimates on
        // the full formatted message, not just file content.
        let att_tokens = tokens::estimate_text_tokens(&text);
        if used_tokens + att_tokens > POST_COMPACT_TOKEN_BUDGET {
            break;
        }
        used_tokens += att_tokens;

        result.push(AttachmentMessage::api(
            coco_types::AttachmentKind::CompactFileReference,
            LlmMessage::user_text(coco_messages::wrapping::wrap_in_system_reminder(&text)),
        ));
    }

    result
}

/// Collect file paths from Read tool calls in preserved messages.
///
/// TS: `collectReadToolFilePaths()` — scans preserved messages for assistant
/// Read tool_use blocks whose tool results are NOT file-unchanged stubs.
/// Files already visible in preserved messages don't need re-injection.
fn collect_read_tool_file_paths(messages: &[Message]) -> HashSet<PathBuf> {
    let read_tool_name = ToolName::Read.as_str();

    // Pass 1: collect tool_use_ids of file-unchanged stubs.
    let stub_ids: HashSet<String> = messages
        .iter()
        .filter_map(|m| match m {
            Message::ToolResult(tr) => {
                if tool_result_starts_with_text(tr, FILE_UNCHANGED_STUB_PREFIX) {
                    Some(tr.tool_use_id.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    // Pass 2: collect file_path from Read tool_use blocks (excluding stubs).
    let mut paths = HashSet::new();
    for msg in messages {
        let Message::Assistant(asst) = msg else {
            continue;
        };
        let LlmMessage::Assistant { content, .. } = &asst.message else {
            continue;
        };
        for part in content {
            let AssistantContentPart::ToolCall(tc) = part else {
                continue;
            };
            if tc.tool_name != read_tool_name || stub_ids.contains(&tc.tool_call_id) {
                continue;
            }
            if let Some(fp) = tc
                .input
                .get("file_path")
                .and_then(serde_json::Value::as_str)
            {
                paths.insert(PathBuf::from(fp));
            }
        }
    }

    paths
}

/// Check if a path should be excluded from post-compact restoration.
///
/// TS: `shouldExcludeFromPostCompactRestore()` — excludes plan files and
/// CLAUDE.md/memory-managed paths (they're re-injected via their own systems).
fn should_exclude_from_restore(path: &Path, cwd: &Path, plan_file: Option<&Path>) -> bool {
    if let Some(plan) = plan_file
        && path == plan
    {
        return true;
    }

    // Exclude CLAUDE.md and memory-managed paths
    if coco_context::memory::is_memory_managed_path(path, cwd) {
        return true;
    }

    // Exclude CLAUDE.md files by name
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_lowercase();
        if lower == "claude.md" || (lower.starts_with("claude.") && lower.ends_with(".md")) {
            return true;
        }
    }

    false
}

/// Check if a tool result message text starts with a given prefix.
fn tool_result_starts_with_text(tr: &coco_types::ToolResultMessage, prefix: &str) -> bool {
    let LlmMessage::Tool { content, .. } = &tr.message else {
        return false;
    };
    for part in content {
        let coco_types::ToolContent::ToolResult(result) = part else {
            continue;
        };
        match &result.output {
            vercel_ai_provider::ToolResultContent::Text { value, .. } => {
                if value.starts_with(prefix) {
                    return true;
                }
            }
            vercel_ai_provider::ToolResultContent::Content { value, .. } => {
                for sub in value {
                    if let vercel_ai_provider::ToolResultContentPart::Text { text, .. } = sub
                        && text.starts_with(prefix)
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
#[path = "post_compact_files.test.rs"]
mod tests;
