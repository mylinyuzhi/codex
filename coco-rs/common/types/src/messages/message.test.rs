use super::*;

#[test]
fn test_message_kind() {
    let msg = Message::Tombstone(TombstoneMessage {
        uuid: Uuid::new_v4(),
        original_kind: MessageKind::User,
    });
    assert_eq!(msg.kind(), MessageKind::Tombstone);
}

#[test]
fn test_stop_reason_serde() {
    let reason = StopReason::EndTurn;
    let json = serde_json::to_string(&reason).unwrap();
    assert_eq!(json, "\"end_turn\"");
}

#[test]
fn test_system_message_level_serde() {
    let level = SystemMessageLevel::Warning;
    let json = serde_json::to_string(&level).unwrap();
    assert_eq!(json, "\"warning\"");
}

fn user_msg(transcript_only: bool) -> Message {
    Message::User(UserMessage {
        message: crate::LlmMessage::user_text("hi"),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: transcript_only,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

#[test]
fn test_transcript_only_user_is_ui_only_not_api() {
    // The model-visibility gate: a transcript-only user message (e.g. a
    // slash-command echo/result with `display: system`) renders but is
    // never sent to the model.
    let v = user_msg(/*transcript_only*/ true).visibility();
    assert!(v.ui, "transcript-only message must still render");
    assert!(!v.api, "transcript-only message must NOT reach the model");
}

#[test]
fn test_normal_user_is_both_visible() {
    let v = user_msg(/*transcript_only*/ false).visibility();
    assert!(v.ui);
    assert!(v.api);
}

#[test]
fn test_message_origin_slash_command_serde() {
    let json = serde_json::to_string(&MessageOrigin::SlashCommand).unwrap();
    assert_eq!(json, "\"slash_command\"");
}
