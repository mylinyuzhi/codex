//! Pure derivation of `RenderedCell`s from engine `Message`s.
//!
//! Hygiene rule: lives in `coco-tui`, not `coco-messages`. The adapter
//! is one-directional (`Message` → cells) and does not mutate the
//! source message. No theme / viewport / hover state is consulted —
//! that lives in the renderer at draw time.
//!
//! See `engine-tui-unified-transcript-plan.md` §2 (Layer Ownership).

use coco_messages::AssistantContent;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_messages::UserContent;
use uuid::Uuid;

use super::transcript_view::CellKind;
use super::transcript_view::RenderedCell;
use super::transcript_view::SystemCellKind;

/// Derive zero or more cells from a single engine `Message`.
///
/// Most variants yield exactly one cell. `Message::Assistant` may
/// yield multiple cells when its content interleaves text / thinking /
/// tool_use blocks. `Message::Tombstone` yields zero (filtered).
pub fn message_to_cells(msg: &Message) -> Vec<RenderedCell> {
    match msg {
        Message::User(user) => {
            let text = extract_user_text(&user.message);
            vec![cell(user.uuid, CellKind::UserText { text })]
        }
        Message::Assistant(asst) => assistant_cells(asst.uuid, &asst.message, &asst.model),
        Message::System(sm) => {
            let uuid = *sm.uuid();
            vec![cell(uuid, CellKind::System(SystemCellKind::from(sm)))]
        }
        Message::ToolResult(tr) => {
            // call_id retrieval depends on internal shape; surface the
            // UUID for now so the cell renders. Renderer can re-fetch
            // call_id from the engine-authoritative Message via
            // `MessageHistory::find_by_uuid` when needed.
            vec![cell(
                tr.uuid,
                CellKind::ToolResult {
                    call_id: String::new(),
                },
            )]
        }
        Message::Attachment(a) => vec![cell(a.uuid, CellKind::Attachment)],
        Message::Progress(_) => Vec::new(),
        Message::Tombstone(_) => Vec::new(),
        Message::ToolUseSummary(s) => vec![cell(
            s.uuid,
            CellKind::ToolUseSummary {
                summary: s.summary.clone(),
            },
        )],
    }
}

fn cell(message_uuid: Uuid, kind: CellKind) -> RenderedCell {
    RenderedCell { message_uuid, kind }
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

fn assistant_cells(uuid: Uuid, msg: &LlmMessage, model: &str) -> Vec<RenderedCell> {
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
        out.push(cell(uuid, kind));
    }
    out
}
