use coco_types::*;
use uuid::Uuid;

use super::*;

fn user_msg(text: &str) -> Message {
    crate::creation::create_user_message(text)
}

fn meta_msg(text: &str) -> Message {
    crate::creation::create_meta_message(text)
}

fn tombstone() -> Message {
    Message::Tombstone(TombstoneMessage {
        uuid: Uuid::new_v4(),
        original_kind: MessageKind::User,
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

#[test]
fn test_filter_tombstones() {
    let msgs = vec![user_msg("hi"), tombstone(), user_msg("bye")];
    let result = filter_tombstones(&msgs);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_filter_meta() {
    let msgs = vec![user_msg("hi"), meta_msg("system"), user_msg("bye")];
    let result = filter_meta(&msgs);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_filter_for_display() {
    let msgs = vec![
        user_msg("hi"),
        meta_msg("system"),
        tombstone(),
        assistant_msg("hello"),
    ];
    let result = filter_for_display(&msgs);
    assert_eq!(result.len(), 2); // user + assistant
}

#[test]
fn test_keep_last_n_turns() {
    let msgs = vec![
        user_msg("turn1"),
        assistant_msg("resp1"),
        user_msg("turn2"),
        assistant_msg("resp2"),
        user_msg("turn3"),
        assistant_msg("resp3"),
    ];
    let result = keep_last_n_turns(&msgs, 2);
    // Should keep last 2 turns (turn2+resp2, turn3+resp3)
    assert_eq!(result.len(), 4);
}

#[test]
fn test_keep_last_n_turns_fewer_than_n() {
    let msgs = vec![user_msg("only"), assistant_msg("one")];
    let result = keep_last_n_turns(&msgs, 5);
    assert_eq!(result.len(), 2); // keep all
}
