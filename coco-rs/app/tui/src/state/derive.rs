//! Pure derivation of `RenderedCell`s from engine `Message`s, plus a
//! lossy back-adapter that materializes `ChatMessage`s for the legacy
//! render pipeline.
//!
//! Hygiene rule: lives in `coco-tui`, not `coco-messages`. The adapter
//! is one-directional (`Message` → cells) and does not mutate the
//! source message. No theme / viewport / hover state is consulted —
//! that lives in the renderer at draw time.
//!
//! See `engine-tui-unified-transcript-plan.md` §2 (Layer Ownership).

use std::sync::Arc;

use coco_messages::AssistantContent;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_messages::SystemMessage;
use coco_messages::UserContent;
use uuid::Uuid;

use super::session::ChatMessage;
use super::session::ChatRole;
use super::session::MessageContent;
use super::session::ToolUseStatus;
use super::transcript_view::CellKind;
use super::transcript_view::RenderedCell;
use super::transcript_view::SystemCellKind;

/// Derive zero or more cells from a single engine `Message`.
///
/// Most variants yield exactly one cell. `Message::Assistant` may
/// yield multiple cells when its content interleaves text / thinking /
/// tool_use blocks. `Message::Tombstone` yields zero (filtered).
pub fn message_to_cells(msg: Arc<Message>) -> Vec<RenderedCell> {
    match &*msg {
        Message::User(user) => {
            let text = extract_user_text(&user.message);
            vec![cell(user.uuid, CellKind::UserText { text }, msg.clone())]
        }
        Message::Assistant(asst) => {
            assistant_cells(asst.uuid, &asst.message, &asst.model, msg.clone())
        }
        Message::System(sm) => {
            let uuid = *sm.uuid();
            vec![cell(
                uuid,
                CellKind::System(SystemCellKind::from(sm)),
                msg.clone(),
            )]
        }
        Message::ToolResult(tr) => {
            // call_id retrieval depends on internal shape; surface the
            // UUID for now so the cell renders. Renderer can re-fetch
            // call_id from `cell.source` (Arc<Message>) when needed.
            vec![cell(
                tr.uuid,
                CellKind::ToolResult {
                    call_id: String::new(),
                },
                msg.clone(),
            )]
        }
        Message::Attachment(a) => vec![cell(a.uuid, CellKind::Attachment, msg.clone())],
        Message::Progress(_) => Vec::new(),
        Message::Tombstone(_) => Vec::new(),
        Message::ToolUseSummary(s) => vec![cell(
            s.uuid,
            CellKind::ToolUseSummary {
                summary: s.summary.clone(),
            },
            msg.clone(),
        )],
    }
}

fn cell(message_uuid: Uuid, kind: CellKind, source: Arc<Message>) -> RenderedCell {
    RenderedCell {
        message_uuid,
        kind,
        source,
    }
}

fn extract_user_text(msg: &LlmMessage) -> String {
    let LlmMessage::User { content, .. } = msg else {
        return String::new();
    };
    let mut buf = String::new();
    for part in content {
        if let UserContent::Text(t) = part {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(&t.text);
        }
    }
    buf
}

fn assistant_cells(
    uuid: Uuid,
    msg: &LlmMessage,
    model: &str,
    source: Arc<Message>,
) -> Vec<RenderedCell> {
    let LlmMessage::Assistant { content, .. } = msg else {
        return Vec::new();
    };
    let mut out: Vec<RenderedCell> = Vec::new();
    for part in content {
        let kind = match part {
            AssistantContent::Text(t) if !t.text.is_empty() => CellKind::AssistantText {
                text: t.text.clone(),
                model: model.to_string(),
            },
            AssistantContent::Reasoning(r) => {
                if r.text.is_empty() {
                    CellKind::AssistantRedactedThinking
                } else {
                    CellKind::AssistantThinking {
                        text: r.text.clone(),
                    }
                }
            }
            AssistantContent::ToolCall(tc) => CellKind::ToolUse {
                call_id: tc.tool_call_id.clone(),
                tool_name: tc.tool_name.clone(),
            },
            _ => continue,
        };
        out.push(cell(uuid, kind, source.clone()));
    }
    out
}

// ─────────────────────────────────────────────────────────────────────
// Legacy back-adapter: RenderedCell → ChatMessage
// ─────────────────────────────────────────────────────────────────────
//
// Until Phase 3d migrates the chat render path to consume `RenderedCell`
// directly, the existing `ChatWidget` + `presentation::transcript`
// pipeline reads `[ChatMessage]`. This adapter materializes a
// ChatMessage shape from cells so the engine-pushed transcript stream
// can flow into the legacy renderer unmodified.
//
// The conversion is lossy in fields CellKind doesn't track
// (`input_preview`, exit codes, file diff hunks, …). The renderer
// degrades gracefully — empty strings render as empty rows rather
// than crashing.

/// Render-friendly `ChatMessage` derived from a `RenderedCell`.
/// Returns `None` for cell kinds that should not surface in the
/// transcript (`Progress`, `Tombstone`, micro-compact boundaries).
pub fn cell_to_chat_message(cell: &RenderedCell) -> Option<ChatMessage> {
    use CellKind as CK;
    use ChatRole as CR;
    use MessageContent as MC;
    use SystemCellKind as SK;

    let (role, content) = match &cell.kind {
        CK::UserText { text } => (CR::User, MC::Text(text.clone())),
        CK::UserAttachment => (
            CR::User,
            MC::Attachment {
                attachment_type: String::new(),
                preview: String::new(),
            },
        ),
        CK::AssistantText { text, .. } => (CR::Assistant, MC::AssistantText(text.clone())),
        CK::AssistantThinking { text } => (
            CR::Assistant,
            MC::Thinking {
                content: text.clone(),
                duration_ms: None,
                reasoning_tokens: None,
            },
        ),
        CK::AssistantRedactedThinking => (CR::Assistant, MC::RedactedThinking),
        CK::ToolUse { call_id, tool_name } => (
            CR::Assistant,
            MC::ToolUse {
                tool_name: tool_name.clone(),
                call_id: call_id.clone(),
                input_preview: String::new(),
                status: ToolUseStatus::Completed,
            },
        ),
        CK::ToolResult { .. } => (
            CR::Tool,
            MC::ToolSuccess {
                tool_name: String::new(),
                output: String::new(),
            },
        ),
        CK::Attachment => (
            CR::User,
            MC::Attachment {
                attachment_type: String::new(),
                preview: String::new(),
            },
        ),
        CK::ToolUseSummary { summary } => (CR::System, MC::SystemText(summary.clone())),
        CK::Progress | CK::Tombstone => return None,
        CK::System(sk) => match sk {
            SK::UserInterruption { for_tool_use } => (
                CR::System,
                MC::InterruptionMarker {
                    for_tool_use: *for_tool_use,
                },
            ),
            SK::CompactBoundary => (CR::System, MC::CompactBoundary),
            SK::MicrocompactBoundary => return None,
            SK::ApiError => extract_api_error(&cell.source)
                .map(|c| (CR::System, c))
                .unwrap_or((CR::System, MC::SystemText(String::new()))),
            SK::Informational => extract_informational(&cell.source)
                .map(|c| (CR::System, c))
                .unwrap_or((CR::System, MC::SystemText(String::new()))),
            SK::LocalCommand => extract_local_command(&cell.source)
                .map(|c| (CR::User, c))
                .unwrap_or((CR::System, MC::SystemText(String::new()))),
            // The remaining SystemCellKind variants don't have a
            // dedicated MessageContent variant — render them as
            // SystemText with a generic body for now. Renderer
            // refinement comes when the chat pipeline switches to
            // CellKind dispatch directly.
            _ => (CR::System, MC::SystemText(String::new())),
        },
    };

    let (is_meta, is_compact_summary, is_visible_in_transcript_only, permission_mode) =
        extract_message_metadata(&cell.source);

    Some(ChatMessage {
        id: cell.message_uuid.to_string(),
        role,
        content,
        is_meta,
        created_at_ms: 0,
        is_compact_summary,
        is_visible_in_transcript_only,
        permission_mode,
    })
}

/// Adapter wrapper: convert every cell that surfaces in the transcript
/// into a ChatMessage. Cells that filter out (Progress / Tombstone /
/// MicrocompactBoundary) are skipped.
pub fn cells_to_chat_messages(cells: &[RenderedCell]) -> Vec<ChatMessage> {
    cells.iter().filter_map(cell_to_chat_message).collect()
}

/// Render-time merge of legacy `session.messages` and transcript-
/// derived cells. Engine-authoritative versions (from transcript)
/// supersede TUI optimistic entries on matching `id`; transcript-only
/// items (cells with no corresponding `session.messages` entry by id)
/// append at the end. Used by the chat renderer call sites in
/// `surface/viewport.rs` and `surface/history_lines.rs`.
///
/// Phase 3c: this is the bridge that lets engine-pushed content
/// (`SystemMessage::UserInterruption`, resume-replayed `Message::User`
/// / `Message::Assistant`, hook outputs, etc.) reach the legacy
/// ChatMessage-based renderer without rewriting every `MessageContent`
/// match arm. Phase 3d will switch renderers to consume `RenderedCell`
/// directly and drop this adapter along with the
/// `ChatMessage` / `MessageContent` types.
pub fn merged_chat_messages(legacy: &[ChatMessage], cells: &[RenderedCell]) -> Vec<ChatMessage> {
    use std::collections::HashMap;
    use std::collections::HashSet;

    let derived = cells_to_chat_messages(cells);
    let derived_by_id: HashMap<&str, &ChatMessage> =
        derived.iter().map(|m| (m.id.as_str(), m)).collect();
    let legacy_ids: HashSet<&str> = legacy.iter().map(|m| m.id.as_str()).collect();

    let mut out: Vec<ChatMessage> = Vec::with_capacity(legacy.len() + derived.len());
    for tui_msg in legacy {
        if let Some(d) = derived_by_id.get(tui_msg.id.as_str()) {
            out.push((*d).clone());
        } else {
            out.push(tui_msg.clone());
        }
    }
    for d in &derived {
        if !legacy_ids.contains(d.id.as_str()) {
            out.push(d.clone());
        }
    }
    out
}

fn extract_message_metadata(
    msg: &Message,
) -> (bool, bool, bool, Option<coco_types::PermissionMode>) {
    match msg {
        Message::User(u) => (
            u.is_virtual,
            u.is_compact_summary,
            u.is_visible_in_transcript_only,
            u.permission_mode,
        ),
        Message::Assistant(_) => (false, false, false, None),
        Message::System(_) => (true, false, false, None),
        Message::Attachment(_) => (false, false, false, None),
        Message::ToolResult(_) => (false, false, false, None),
        Message::Progress(_) | Message::Tombstone(_) | Message::ToolUseSummary(_) => {
            (false, false, false, None)
        }
    }
}

fn extract_api_error(msg: &Message) -> Option<MessageContent> {
    let Message::System(SystemMessage::ApiError(e)) = msg else {
        return None;
    };
    Some(MessageContent::ApiError {
        error: e.error.clone(),
        retryable: false,
        status_code: e.status_code,
    })
}

fn extract_informational(msg: &Message) -> Option<MessageContent> {
    let Message::System(SystemMessage::Informational(info)) = msg else {
        return None;
    };
    let text = if info.title.is_empty() {
        info.message.clone()
    } else {
        format!("{}: {}", info.title, info.message)
    };
    Some(MessageContent::SystemText(text))
}

/// Project `SystemMessage::LocalCommand { command, output }` into the
/// legacy `MessageContent::BashOutput` shape so the existing renderer
/// surfaces both the input + output of `!cmd` from a single engine
/// message. Mirrors what the TUI used to do via two separate
/// `add_message(user_bash_input + user_bash_output)` calls prior to
/// `engine-tui-unified-transcript-plan.md` Commit 2.
fn extract_local_command(msg: &Message) -> Option<MessageContent> {
    let Message::System(SystemMessage::LocalCommand(lc)) = msg else {
        return None;
    };
    let body = if lc.output.is_empty() {
        format!("$ {}", lc.command)
    } else {
        format!("$ {}\n{}", lc.command, lc.output)
    };
    Some(MessageContent::BashOutput {
        output: body,
        exit_code: 0,
    })
}
