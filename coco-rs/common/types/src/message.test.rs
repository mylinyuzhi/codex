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
