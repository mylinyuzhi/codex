use super::*;
use crate::types::AttachmentType;
use crate::types::ContentBlock;
use crate::types::ReminderMessage;

fn test_reminder(content: &str) -> SystemReminder {
    SystemReminder::new(AttachmentType::ChangedFiles, content)
}

#[test]
fn test_inject_reminders() {
    let reminders = vec![
        test_reminder("File a.rs changed"),
        test_reminder("File b.rs changed"),
    ];

    let injected = inject_reminders(reminders);
    assert_eq!(injected.len(), 2);
    assert!(injected[0].contains("<system-reminder>"));
    assert!(injected[0].contains("File a.rs changed"));
}

#[test]
fn test_inject_empty() {
    let injected = inject_reminders(vec![]);
    assert!(injected.is_empty());
}

#[test]
fn test_inject_skips_multi_message() {
    let messages = vec![
        ReminderMessage::assistant(vec![ContentBlock::tool_use(
            "test-id",
            "Read",
            serde_json::json!({}),
        )]),
        ReminderMessage::user(vec![ContentBlock::tool_result("test-id", "content")]),
    ];
    let reminders = vec![
        test_reminder("Text reminder"),
        SystemReminder::messages(AttachmentType::AlreadyReadFile, messages),
    ];

    let injected = inject_reminders(reminders);
    // Only the text reminder should be included
    assert_eq!(injected.len(), 1);
    assert!(injected[0].contains("Text reminder"));
}

#[test]
fn test_combine_reminders() {
    let reminders = vec![
        test_reminder("First reminder"),
        test_reminder("Second reminder"),
    ];

    let combined = combine_reminders(reminders);
    assert!(combined.is_some());

    let content = combined.expect("content");
    assert!(content.contains("First reminder"));
    assert!(content.contains("Second reminder"));
    assert!(content.contains("\n\n")); // Separated by double newline
}

#[test]
fn test_combine_empty() {
    let combined = combine_reminders(vec![]);
    assert!(combined.is_none());
}

#[test]
fn test_combine_only_multi_message() {
    let messages = vec![ReminderMessage::assistant(vec![ContentBlock::tool_use(
        "test-id",
        "Read",
        serde_json::json!({}),
    )])];
    let reminders = vec![SystemReminder::messages(
        AttachmentType::AlreadyReadFile,
        messages,
    )];

    let combined = combine_reminders(reminders);
    assert!(combined.is_none());
}

#[test]
fn test_injection_stats() {
    let reminders = vec![
        SystemReminder::new(AttachmentType::ChangedFiles, "change 1"),
        SystemReminder::new(AttachmentType::ChangedFiles, "change 2"),
        SystemReminder::new(AttachmentType::PlanModeEnter, "plan instructions"),
    ];

    let stats = InjectionStats::from_reminders(&reminders);
    assert_eq!(stats.total_count, 3);
    assert_eq!(stats.by_type.get("changed_files"), Some(&2));
    assert_eq!(stats.by_type.get("plan_mode_enter"), Some(&1));
    assert_eq!(stats.multi_message_count, 0);
}

#[test]
fn test_injection_stats_with_multi_message() {
    let messages = vec![
        ReminderMessage::assistant(vec![ContentBlock::tool_use(
            "test-id",
            "Read",
            serde_json::json!({}),
        )]),
        ReminderMessage::user(vec![ContentBlock::tool_result("test-id", "file content")]),
    ];
    let reminders = vec![
        SystemReminder::new(AttachmentType::ChangedFiles, "change 1"),
        SystemReminder::messages(AttachmentType::AlreadyReadFile, messages),
    ];

    let stats = InjectionStats::from_reminders(&reminders);
    assert_eq!(stats.total_count, 2);
    assert_eq!(stats.multi_message_count, 1);
    assert_eq!(stats.by_type.get("already_read_file"), Some(&1));
}

// ======== Tests for create_injected_messages ========

#[test]
fn test_create_injected_messages_text() {
    let reminders = vec![
        test_reminder("File changed"),
        SystemReminder::new(AttachmentType::PlanModeEnter, "Plan mode active"),
    ];

    let injected = create_injected_messages(reminders);
    assert_eq!(injected.len(), 2);

    // Check first message
    match &injected[0] {
        InjectedMessage::UserText { content, is_meta } => {
            assert!(content.contains("<system-reminder>"));
            assert!(content.contains("File changed"));
            assert!(*is_meta);
        }
        _ => panic!("Expected UserText"),
    }
}

#[test]
fn test_create_injected_messages_multi_message() {
    let messages = vec![
        ReminderMessage::assistant(vec![ContentBlock::tool_use(
            "test-id",
            "Read",
            serde_json::json!({"file_path": "/test.rs"}),
        )]),
        ReminderMessage::user(vec![ContentBlock::tool_result(
            "test-id",
            "[Previously read: 10 lines]",
        )]),
    ];
    let reminders = vec![SystemReminder::messages(
        AttachmentType::AlreadyReadFile,
        messages,
    )];

    let injected = create_injected_messages(reminders);
    assert_eq!(injected.len(), 2);

    // Check assistant message with tool_use
    match &injected[0] {
        InjectedMessage::AssistantBlocks { blocks, is_meta } => {
            assert_eq!(blocks.len(), 1);
            assert!(*is_meta);
            match &blocks[0] {
                InjectedBlock::ToolUse { id, name, input } => {
                    assert_eq!(id, "test-id");
                    assert_eq!(name, "Read");
                    assert_eq!(input["file_path"], "/test.rs");
                }
                _ => panic!("Expected ToolUse block"),
            }
        }
        _ => panic!("Expected AssistantBlocks"),
    }

    // Check user message with tool_result
    match &injected[1] {
        InjectedMessage::UserBlocks { blocks, is_meta } => {
            assert_eq!(blocks.len(), 1);
            assert!(*is_meta);
            match &blocks[0] {
                InjectedBlock::ToolResult {
                    tool_use_id,
                    content,
                } => {
                    assert_eq!(tool_use_id, "test-id");
                    assert!(content.contains("Previously read"));
                }
                _ => panic!("Expected ToolResult block"),
            }
        }
        _ => panic!("Expected UserBlocks"),
    }
}

#[test]
fn test_create_injected_messages_mixed() {
    let messages = vec![
        ReminderMessage::assistant(vec![ContentBlock::tool_use(
            "read-1",
            "Read",
            serde_json::json!({}),
        )]),
        ReminderMessage::user(vec![ContentBlock::tool_result("read-1", "content")]),
    ];
    let reminders = vec![
        test_reminder("Text reminder"),
        SystemReminder::messages(AttachmentType::AlreadyReadFile, messages),
    ];

    let injected = create_injected_messages(reminders);
    assert_eq!(injected.len(), 3); // 1 text + 2 messages

    // First should be text
    assert!(matches!(&injected[0], InjectedMessage::UserText { .. }));
    // Then assistant blocks
    assert!(matches!(
        &injected[1],
        InjectedMessage::AssistantBlocks { .. }
    ));
    // Then user blocks
    assert!(matches!(&injected[2], InjectedMessage::UserBlocks { .. }));
}

#[test]
fn test_create_injected_messages_empty() {
    let injected = create_injected_messages(vec![]);
    assert!(injected.is_empty());
}