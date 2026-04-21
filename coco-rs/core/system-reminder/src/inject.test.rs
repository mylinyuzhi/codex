use super::*;
use crate::types::AttachmentType;
use crate::types::ContentBlock;
use crate::types::MessageRole;
use crate::types::ReminderMessage;
use crate::types::ReminderOutput;
use crate::types::SystemReminder;
use pretty_assertions::assert_eq;

// ── create_injected_messages ──

#[test]
fn text_reminder_becomes_user_text_wrapped() {
    let r = SystemReminder::new(AttachmentType::PlanMode, "hello");
    let out = create_injected_messages(vec![r]);
    assert_eq!(out.len(), 1);
    match &out[0] {
        InjectedMessage::UserText {
            kind,
            content,
            is_meta,
        } => {
            assert_eq!(*kind, coco_types::AttachmentKind::PlanMode);
            assert_eq!(content, "<system-reminder>\nhello\n</system-reminder>");
            assert!(is_meta);
        }
        _ => panic!("expected UserText"),
    }
}

#[test]
fn silent_reminder_is_filtered_out() {
    let r = SystemReminder::new(AttachmentType::PlanMode, "x").silent();
    assert_eq!(create_injected_messages(vec![r]).len(), 0);
}

#[test]
fn empty_text_output_is_filtered_out() {
    let r = SystemReminder::new(AttachmentType::PlanMode, "");
    assert_eq!(create_injected_messages(vec![r]).len(), 0);
}

#[test]
fn empty_reminder_batch_produces_nothing() {
    assert_eq!(create_injected_messages(Vec::new()).len(), 0);
}

#[test]
fn messages_output_produces_user_and_assistant_blocks() {
    let msgs = vec![
        ReminderMessage::assistant(vec![ContentBlock::ToolUse {
            id: "tool-1".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({"path": "foo.rs"}),
        }]),
        ReminderMessage {
            role: MessageRole::User,
            blocks: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-1".to_string(),
                content: "file contents".to_string(),
            }],
            is_meta: true,
        },
    ];
    let r = SystemReminder::messages(AttachmentType::PlanMode, msgs);
    let out = create_injected_messages(vec![r]);
    assert_eq!(out.len(), 2);
    matches!(out[0], InjectedMessage::AssistantBlocks { .. });
    matches!(out[1], InjectedMessage::UserBlocks { .. });
}

#[test]
fn messages_output_wraps_text_blocks_per_tag() {
    let msgs = vec![ReminderMessage::user_text("note")];
    let r = SystemReminder::messages(AttachmentType::PlanMode, msgs);
    let out = create_injected_messages(vec![r]);
    assert_eq!(out.len(), 1);
    match &out[0] {
        InjectedMessage::UserBlocks { blocks, .. } => {
            assert_eq!(blocks.len(), 1);
            match &blocks[0] {
                InjectedBlock::Text(t) => {
                    assert_eq!(t, "<system-reminder>\nnote\n</system-reminder>");
                }
                _ => panic!("expected Text block"),
            }
        }
        _ => panic!("expected UserBlocks"),
    }
}

#[test]
fn model_attachment_becomes_json_wrapped_usertext() {
    let payload = serde_json::json!({"foo": "bar", "n": 42});
    let r = SystemReminder {
        attachment_type: AttachmentType::PlanMode,
        output: ReminderOutput::ModelAttachment { payload },
        is_meta: true,
        is_silent: false,
        metadata: None,
    };
    let out = create_injected_messages(vec![r]);
    assert_eq!(out.len(), 1);
    match &out[0] {
        InjectedMessage::UserText { content, .. } => {
            assert!(content.starts_with("<system-reminder>\n{"));
            assert!(content.contains(r#""foo": "bar""#));
            assert!(content.ends_with("}\n</system-reminder>"));
        }
        _ => panic!("expected UserText"),
    }
}

#[test]
fn multiple_reminders_preserve_order() {
    let r1 = SystemReminder::new(AttachmentType::PlanMode, "one");
    let r2 = SystemReminder::new(AttachmentType::PlanModeExit, "two");
    let out = create_injected_messages(vec![r1, r2]);
    assert_eq!(out.len(), 2);
    match (&out[0], &out[1]) {
        (
            InjectedMessage::UserText { content: c1, .. },
            InjectedMessage::UserText { content: c2, .. },
        ) => {
            assert!(c1.contains("one"));
            assert!(c2.contains("two"));
        }
        _ => panic!("expected two UserText in order"),
    }
}

// ── inject_reminders ──

#[test]
fn inject_text_reminder_produces_attachment_message_with_is_meta() {
    use coco_types::Message;
    let mut history: Vec<Message> = Vec::new();
    inject_reminders(
        vec![SystemReminder::new(AttachmentType::PlanMode, "hi")],
        &mut history,
    );
    assert_eq!(history.len(), 1);
    match &history[0] {
        Message::Attachment(a) => {
            assert_eq!(a.kind, coco_types::AttachmentKind::PlanMode);
            assert!(a.as_api_message().is_some());
        }
        _ => panic!("expected Attachment variant, got {:?}", history[0].kind()),
    }
}

#[test]
fn inject_empty_batch_leaves_history_unchanged() {
    use coco_types::Message;
    let mut history: Vec<Message> = Vec::new();
    inject_reminders(Vec::new(), &mut history);
    assert_eq!(history.len(), 0);
}

#[test]
fn inject_silent_reminder_does_not_append() {
    use coco_types::Message;
    let mut history: Vec<Message> = Vec::new();
    inject_reminders(
        vec![SystemReminder::new(AttachmentType::PlanMode, "x").silent()],
        &mut history,
    );
    assert_eq!(history.len(), 0);
}

#[test]
fn inject_user_blocks_produces_user_message_with_system_injected_origin() {
    use coco_types::Message;
    let msgs = vec![ReminderMessage::user_text("note")];
    let r = SystemReminder::messages(AttachmentType::PlanMode, msgs);
    let mut history: Vec<Message> = Vec::new();
    inject_reminders(vec![r], &mut history);
    assert_eq!(history.len(), 1);
    match &history[0] {
        // Post-Phase-2: multi-block reminder messages land as
        // Message::Attachment with Api body + kind.
        Message::Attachment(a) => {
            assert_eq!(a.kind, coco_types::AttachmentKind::PlanMode);
        }
        _ => panic!("expected Attachment variant"),
    }
}

#[test]
fn inject_multiple_reminders_appends_in_order() {
    use coco_types::Message;
    let mut history: Vec<Message> = Vec::new();
    inject_reminders(
        vec![
            SystemReminder::new(AttachmentType::PlanMode, "a"),
            SystemReminder::new(AttachmentType::PlanModeExit, "b"),
        ],
        &mut history,
    );
    assert_eq!(history.len(), 2);
    // Both are Attachment messages.
    assert!(matches!(history[0], Message::Attachment(_)));
    assert!(matches!(history[1], Message::Attachment(_)));
}
