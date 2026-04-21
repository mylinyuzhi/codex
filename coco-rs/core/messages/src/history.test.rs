use coco_types::*;
use pretty_assertions::assert_eq;
use uuid::Uuid;

use super::*;

fn user_msg(text: &str) -> Message {
    Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
    })
}

fn assistant_msg(text: &str) -> Message {
    Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![AssistantContent::Text(TextContent {
                text: text.into(),
                provider_metadata: None,
            })],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".into(),
        stop_reason: Some(StopReason::EndTurn),
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    })
}

fn assistant_msg_empty() -> Message {
    Message::Assistant(AssistantMessage {
        message: LlmMessage::Assistant {
            content: vec![],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        model: "test".into(),
        stop_reason: None,
        usage: None,
        cost_usd: None,
        request_id: None,
        api_error: None,
    })
}

#[test]
fn test_len_and_is_empty() {
    let mut history = MessageHistory::new();
    assert!(history.is_empty());
    assert_eq!(history.len(), 0);

    history.push(user_msg("hello"));
    assert!(!history.is_empty());
    assert_eq!(history.len(), 1);
}

#[test]
fn test_as_slice() {
    let mut history = MessageHistory::new();
    history.push(user_msg("a"));
    history.push(assistant_msg("b"));
    let slice = history.as_slice();
    assert_eq!(slice.len(), 2);
}

#[test]
fn test_last_assistant_text_found() {
    let mut history = MessageHistory::new();
    history.push(user_msg("hello"));
    history.push(assistant_msg("first"));
    history.push(user_msg("more"));
    history.push(assistant_msg("second"));
    assert_eq!(history.last_assistant_text(), Some("second".to_string()));
}

#[test]
fn test_last_assistant_text_none() {
    let mut history = MessageHistory::new();
    history.push(user_msg("hello"));
    assert_eq!(history.last_assistant_text(), None);
}

#[test]
fn test_last_assistant_text_empty_content() {
    let mut history = MessageHistory::new();
    history.push(assistant_msg_empty());
    assert_eq!(history.last_assistant_text(), None);
}

#[test]
fn test_last_assistant_text_empty_history() {
    let history = MessageHistory::new();
    assert_eq!(history.last_assistant_text(), None);
}

#[test]
fn test_count_by_kind() {
    let mut history = MessageHistory::new();
    history.push(user_msg("a"));
    history.push(user_msg("b"));
    history.push(assistant_msg("c"));
    assert_eq!(history.count_by_kind(MessageKind::User), 2);
    assert_eq!(history.count_by_kind(MessageKind::Assistant), 1);
    assert_eq!(history.count_by_kind(MessageKind::System), 0);
}

#[test]
fn test_count_by_kind_empty() {
    let history = MessageHistory::new();
    assert_eq!(history.count_by_kind(MessageKind::User), 0);
}

#[test]
fn test_clear() {
    let mut history = MessageHistory::new();
    history.push(user_msg("a"));
    history.push(assistant_msg("b"));
    history.clear();
    assert!(history.is_empty());
    assert_eq!(history.len(), 0);
}

#[test]
fn test_truncate_keep_last_basic() {
    let mut history = MessageHistory::new();
    let msg_a = user_msg("a");
    let msg_b = assistant_msg("b");
    let msg_c = user_msg("c");
    let uuid_c = *msg_c.uuid().expect("uuid");
    history.push(msg_a);
    history.push(msg_b);
    history.push(msg_c);
    history.truncate_keep_last(2);
    assert_eq!(history.len(), 2);
    // The UUID index should still work for retained messages.
    assert!(history.find_by_uuid(&uuid_c).is_some());
}

#[test]
fn test_truncate_keep_last_n_larger() {
    let mut history = MessageHistory::new();
    history.push(user_msg("a"));
    history.truncate_keep_last(10);
    assert_eq!(history.len(), 1);
}

#[test]
fn test_truncate_keep_last_zero() {
    let mut history = MessageHistory::new();
    history.push(user_msg("a"));
    history.push(user_msg("b"));
    history.truncate_keep_last(0);
    assert!(history.is_empty());
}

#[test]
fn test_truncate_keep_last_empty() {
    let mut history = MessageHistory::new();
    history.truncate_keep_last(5);
    assert!(history.is_empty());
}

#[test]
fn test_find_by_uuid() {
    let mut history = MessageHistory::new();
    let msg = user_msg("findme");
    let uuid = *msg.uuid().expect("uuid");
    history.push(msg);
    assert!(history.find_by_uuid(&uuid).is_some());
    assert!(history.find_by_uuid(&Uuid::new_v4()).is_none());
}
