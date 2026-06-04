use super::is_slash_command_origin;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_messages::MessageOrigin;
use coco_messages::UserMessage;
use std::sync::Arc;
use uuid::Uuid;

fn user_cell(text: &str, origin: Option<MessageOrigin>) -> RenderedCell {
    let msg = Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: origin.is_some(),
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin,
        parent_tool_use_id: None,
    });
    RenderedCell {
        message_uuid: Uuid::new_v4(),
        kind: CellKind::UserText {
            text: text.to_string(),
        },
        source: Arc::new(msg),
    }
}

#[test]
fn slash_origin_gates_command_pill_rendering() {
    let echo = "<command-name>/help</command-name>\n<command-args></command-args>";
    // Genuine slash echo (origin stamped) → eligible for the `❯ /cmd` pill.
    assert!(is_slash_command_origin(&user_cell(
        echo,
        Some(MessageOrigin::SlashCommand)
    )));
    // Identical text typed by a user (no slash origin) → NOT a pill; it
    // renders as plain user text so a raw `<command-name>` substring is never
    // mistaken for a command invocation.
    assert!(!is_slash_command_origin(&user_cell(echo, None)));
}
