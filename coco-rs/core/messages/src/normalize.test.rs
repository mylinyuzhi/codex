use coco_types::*;
use uuid::Uuid;

use super::ensure_user_first;
use super::merge_consecutive_assistant_messages;
use super::merge_consecutive_user_messages;
use super::normalize_messages_for_api;
use super::strip_images_from_messages;
use super::strip_signature_blocks;
use super::to_llm_prompt;

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
        parent_tool_use_id: None,
    })
}

fn virtual_msg(text: &str) -> Message {
    Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: true,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
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

fn tombstone_msg() -> Message {
    Message::Tombstone(TombstoneMessage {
        uuid: Uuid::new_v4(),
        original_kind: MessageKind::User,
    })
}

#[test]
fn test_filters_virtual_messages() {
    let msgs = vec![user_msg("hello"), virtual_msg("ghost"), assistant_msg("hi")];
    let result = normalize_messages_for_api(&msgs);
    assert_eq!(result.len(), 2); // virtual filtered out
}

#[test]
fn test_filters_tombstones() {
    let msgs = vec![user_msg("hello"), tombstone_msg(), assistant_msg("hi")];
    let result = normalize_messages_for_api(&msgs);
    assert_eq!(result.len(), 2); // tombstone filtered out
}

#[test]
fn test_merges_consecutive_user_messages() {
    let msgs = vec![user_msg("hello"), user_msg("world"), assistant_msg("hi")];
    let result = normalize_messages_for_api(&msgs);
    // Two user messages merged into one, plus assistant = 2
    assert_eq!(result.len(), 2);
}

#[test]
fn test_ensures_starts_with_user() {
    let msgs = vec![assistant_msg("hi")];
    let result = normalize_messages_for_api(&msgs);
    assert!(matches!(&result[0], LlmMessage::User { .. }));
}

#[test]
fn test_empty_input() {
    let result = normalize_messages_for_api(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_progress_messages_filtered() {
    let msgs = vec![
        user_msg("hello"),
        Message::Progress(ProgressMessage {
            tool_use_id: "test".into(),
            data: serde_json::Value::Null,
            parent_message_uuid: None,
        }),
        assistant_msg("hi"),
    ];
    let result = normalize_messages_for_api(&msgs);
    assert_eq!(result.len(), 2); // progress filtered
}

// === merge_consecutive_user_messages ===

#[test]
fn test_merge_consecutive_user_messages_basic() {
    let mut msgs = vec![user_msg("hello"), user_msg("world"), assistant_msg("hi")];
    merge_consecutive_user_messages(&mut msgs);
    assert_eq!(msgs.len(), 2);
    // First message should have merged content.
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            assert_eq!(content.len(), 2); // two text parts merged
        } else {
            panic!("expected User LlmMessage");
        }
    } else {
        panic!("expected User message");
    }
}

#[test]
fn test_merge_consecutive_user_messages_empty() {
    let mut msgs: Vec<Message> = vec![];
    merge_consecutive_user_messages(&mut msgs);
    assert!(msgs.is_empty());
}

#[test]
fn test_merge_consecutive_user_messages_single() {
    let mut msgs = vec![user_msg("only")];
    merge_consecutive_user_messages(&mut msgs);
    assert_eq!(msgs.len(), 1);
}

#[test]
fn test_merge_consecutive_user_messages_no_merge() {
    let mut msgs = vec![user_msg("a"), assistant_msg("b"), user_msg("c")];
    merge_consecutive_user_messages(&mut msgs);
    assert_eq!(msgs.len(), 3);
}

#[test]
fn test_merge_consecutive_user_messages_three() {
    let mut msgs = vec![user_msg("a"), user_msg("b"), user_msg("c")];
    merge_consecutive_user_messages(&mut msgs);
    assert_eq!(msgs.len(), 1);
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            assert_eq!(content.len(), 3);
        } else {
            panic!("expected User LlmMessage");
        }
    } else {
        panic!("expected User message");
    }
}

// === merge_consecutive_assistant_messages ===

#[test]
fn test_merge_consecutive_assistant_messages_basic() {
    let mut msgs = vec![user_msg("hi"), assistant_msg("a"), assistant_msg("b")];
    merge_consecutive_assistant_messages(&mut msgs);
    assert_eq!(msgs.len(), 2);
    if let Message::Assistant(a) = &msgs[1] {
        if let LlmMessage::Assistant { content, .. } = &a.message {
            assert_eq!(content.len(), 2);
        } else {
            panic!("expected Assistant LlmMessage");
        }
    } else {
        panic!("expected Assistant message");
    }
}

#[test]
fn test_merge_consecutive_assistant_messages_empty() {
    let mut msgs: Vec<Message> = vec![];
    merge_consecutive_assistant_messages(&mut msgs);
    assert!(msgs.is_empty());
}

#[test]
fn test_merge_consecutive_assistant_messages_no_merge() {
    let mut msgs = vec![assistant_msg("a"), user_msg("b"), assistant_msg("c")];
    merge_consecutive_assistant_messages(&mut msgs);
    assert_eq!(msgs.len(), 3);
}

// === strip_images_from_messages ===

fn user_msg_with_image() -> Message {
    use coco_types::UserContent;
    use vercel_ai_provider::DataContent;
    use vercel_ai_provider::FilePart;
    use vercel_ai_provider::TextPart;

    Message::User(UserMessage {
        message: LlmMessage::User {
            content: vec![
                UserContent::Text(TextPart::new("caption")),
                UserContent::File(FilePart::new(
                    DataContent::from_base64("abc123"),
                    "image/png",
                )),
            ],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

fn user_msg_image_only() -> Message {
    use coco_types::UserContent;
    use vercel_ai_provider::DataContent;
    use vercel_ai_provider::FilePart;

    Message::User(UserMessage {
        message: LlmMessage::User {
            content: vec![UserContent::File(FilePart::new(
                DataContent::from_base64("abc123"),
                "image/jpeg",
            ))],
            provider_options: None,
        },
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

#[test]
fn test_strip_images_keeps_text() {
    let mut msgs = vec![user_msg_with_image()];
    strip_images_from_messages(&mut msgs);
    assert_eq!(msgs.len(), 1);
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            assert_eq!(content.len(), 1);
            assert!(matches!(content[0], coco_types::UserContent::Text(_)));
        } else {
            panic!("expected User LlmMessage");
        }
    } else {
        panic!("expected User message");
    }
}

#[test]
fn test_strip_images_removes_empty_messages() {
    let mut msgs = vec![user_msg("keep"), user_msg_image_only()];
    strip_images_from_messages(&mut msgs);
    assert_eq!(msgs.len(), 1);
}

#[test]
fn test_strip_images_empty_input() {
    let mut msgs: Vec<Message> = vec![];
    strip_images_from_messages(&mut msgs);
    assert!(msgs.is_empty());
}

#[test]
fn test_strip_images_preserves_assistant() {
    let mut msgs = vec![user_msg_with_image(), assistant_msg("hi")];
    strip_images_from_messages(&mut msgs);
    assert_eq!(msgs.len(), 2);
}

// === strip_signature_blocks ===

fn user_msg_with_sig(text: &str) -> Message {
    Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: false,
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin: None,
        parent_tool_use_id: None,
    })
}

#[test]
fn test_strip_signature_basic() {
    let mut msgs = vec![user_msg_with_sig("Hello\n-- \nJohn Doe")];
    strip_signature_blocks(&mut msgs);
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            if let coco_types::UserContent::Text(t) = &content[0] {
                assert_eq!(t.text, "Hello");
            } else {
                panic!("expected text");
            }
        } else {
            panic!("expected user llm msg");
        }
    } else {
        panic!("expected user msg");
    }
}

#[test]
fn test_strip_signature_no_sig() {
    let mut msgs = vec![user_msg_with_sig("No signature here")];
    strip_signature_blocks(&mut msgs);
    if let Message::User(u) = &msgs[0] {
        if let LlmMessage::User { content, .. } = &u.message {
            if let coco_types::UserContent::Text(t) = &content[0] {
                assert_eq!(t.text, "No signature here");
            } else {
                panic!("expected text");
            }
        } else {
            panic!("expected user llm msg");
        }
    } else {
        panic!("expected user msg");
    }
}

#[test]
fn test_strip_signature_empty() {
    let mut msgs: Vec<Message> = vec![];
    strip_signature_blocks(&mut msgs);
    assert!(msgs.is_empty());
}

// === ensure_user_first ===

#[test]
fn test_ensure_user_first_already_user() {
    let mut msgs = vec![user_msg("hello"), assistant_msg("hi")];
    ensure_user_first(&mut msgs);
    assert_eq!(msgs.len(), 2);
}

#[test]
fn test_ensure_user_first_prepends() {
    let mut msgs = vec![assistant_msg("hi")];
    ensure_user_first(&mut msgs);
    assert_eq!(msgs.len(), 2);
    assert!(matches!(msgs[0], Message::User(_)));
}

#[test]
fn test_ensure_user_first_empty() {
    let mut msgs: Vec<Message> = vec![];
    ensure_user_first(&mut msgs);
    assert!(msgs.is_empty());
}

// === to_llm_prompt ===

#[test]
fn test_to_llm_prompt_basic() {
    let msgs = vec![user_msg("hello"), assistant_msg("hi")];
    let prompt = to_llm_prompt(&msgs);
    assert_eq!(prompt.len(), 2);
    assert!(matches!(prompt[0], LlmMessage::User { .. }));
    assert!(matches!(prompt[1], LlmMessage::Assistant { .. }));
}

#[test]
fn test_to_llm_prompt_skips_system_and_progress() {
    let msgs = vec![
        user_msg("hello"),
        Message::Progress(ProgressMessage {
            tool_use_id: "test".into(),
            data: serde_json::Value::Null,
            parent_message_uuid: None,
        }),
        tombstone_msg(),
        assistant_msg("hi"),
    ];
    let prompt = to_llm_prompt(&msgs);
    // Progress, Tombstone are skipped.
    assert_eq!(prompt.len(), 2);
}

#[test]
fn test_to_llm_prompt_empty() {
    let prompt = to_llm_prompt(&[]);
    assert!(prompt.is_empty());
}
