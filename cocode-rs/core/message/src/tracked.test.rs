use super::*;

#[test]
fn test_user_message() {
    let msg = TrackedMessage::user("Hello", "turn-1");
    assert_eq!(msg.role(), Role::User);
    assert_eq!(msg.turn_id, "turn-1");
    assert_eq!(msg.text(), "Hello");
    assert!(!msg.is_tombstoned());
    assert!(matches!(msg.source, MessageSource::User));
}

#[test]
fn test_assistant_message() {
    let msg = TrackedMessage::assistant("Hi there", "turn-1", Some("req-123".to_string()));
    assert_eq!(msg.role(), Role::Assistant);
    assert!(matches!(
        msg.source,
        MessageSource::Assistant { request_id: Some(ref id) } if id == "req-123"
    ));
}

#[test]
fn test_tool_result_message() {
    let msg = TrackedMessage::tool_result("call-1", "result data", "turn-1");
    assert!(matches!(
        msg.source,
        MessageSource::Tool { call_id: ref id } if id == "call-1"
    ));
}

#[test]
fn test_tombstoning() {
    let mut msg = TrackedMessage::user("Hello", "turn-1");
    assert!(!msg.is_tombstoned());

    msg.tombstone();
    assert!(msg.is_tombstoned());
}

#[test]
fn test_uuid_uniqueness() {
    let msg1 = TrackedMessage::user("Hello", "turn-1");
    let msg2 = TrackedMessage::user("Hello", "turn-1");
    assert_ne!(msg1.uuid, msg2.uuid);
}

#[test]
fn test_into_message() {
    let tracked = TrackedMessage::user("Hello", "turn-1");
    let message: Message = tracked.into();
    assert_eq!(message.role, Role::User);
}

#[test]
fn test_assistant_with_content() {
    let content = vec![
        ContentBlock::text("Let me help"),
        ContentBlock::tool_use("call_1", "get_weather", serde_json::json!({"city": "NYC"})),
    ];
    let msg = TrackedMessage::assistant_with_content(content, "turn-1", None);
    assert!(msg.has_tool_calls());
    assert_eq!(msg.tool_calls().len(), 1);
}

#[test]
fn test_system_reminder_message() {
    let msg = TrackedMessage::system_reminder(
        "<system-reminder>File changed</system-reminder>",
        "changed_files",
        "turn-1",
    );
    assert_eq!(msg.role(), Role::User); // System reminders are sent as user messages
    assert!(msg.is_meta()); // But marked as meta
    assert!(matches!(
        msg.source,
        MessageSource::SystemReminder { reminder_type: ref t } if t == "changed_files"
    ));
}

#[test]
fn test_is_meta_default() {
    // Regular messages should not be meta
    let msg = TrackedMessage::user("Hello", "turn-1");
    assert!(!msg.is_meta());

    let msg = TrackedMessage::assistant("Hi", "turn-1", None);
    assert!(!msg.is_meta());
}

#[test]
fn test_set_meta() {
    let mut msg = TrackedMessage::user("Hello", "turn-1");
    assert!(!msg.is_meta());

    msg.set_meta(true);
    assert!(msg.is_meta());

    msg.set_meta(false);
    assert!(!msg.is_meta());
}

#[test]
fn test_new_meta() {
    let msg =
        TrackedMessage::new_meta(Message::user("meta content"), "turn-1", MessageSource::User);
    assert!(msg.is_meta());
}
