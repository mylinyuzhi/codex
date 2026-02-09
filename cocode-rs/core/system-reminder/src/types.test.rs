use super::*;

#[test]
fn test_xml_tag_names() {
    assert_eq!(XmlTag::SystemReminder.tag_name(), Some("system-reminder"));
    assert_eq!(
        XmlTag::SystemNotification.tag_name(),
        Some("system-notification")
    );
    assert_eq!(XmlTag::NewDiagnostics.tag_name(), Some("new-diagnostics"));
    assert_eq!(XmlTag::SessionMemory.tag_name(), Some("session-memory"));
    assert_eq!(XmlTag::None.tag_name(), None);
}

#[test]
fn test_attachment_type_tiers() {
    // Core tier
    assert_eq!(AttachmentType::ChangedFiles.tier(), ReminderTier::Core);
    assert_eq!(AttachmentType::PlanModeEnter.tier(), ReminderTier::Core);
    assert_eq!(AttachmentType::NestedMemory.tier(), ReminderTier::Core);

    // MainAgentOnly tier
    assert_eq!(
        AttachmentType::LspDiagnostics.tier(),
        ReminderTier::MainAgentOnly
    );
    assert_eq!(
        AttachmentType::TodoReminders.tier(),
        ReminderTier::MainAgentOnly
    );

    // UserPrompt tier
    assert_eq!(
        AttachmentType::AtMentionedFiles.tier(),
        ReminderTier::UserPrompt
    );
}

#[test]
fn test_attachment_type_xml_tags() {
    assert_eq!(
        AttachmentType::ChangedFiles.xml_tag(),
        XmlTag::SystemReminder
    );
    assert_eq!(
        AttachmentType::LspDiagnostics.xml_tag(),
        XmlTag::NewDiagnostics
    );
    assert_eq!(
        AttachmentType::SessionMemoryContent.xml_tag(),
        XmlTag::SessionMemory
    );
}

#[test]
fn test_system_reminder_creation() {
    let reminder = SystemReminder::new(
        AttachmentType::ChangedFiles,
        "File foo.rs has been modified",
    );

    assert_eq!(reminder.attachment_type, AttachmentType::ChangedFiles);
    assert_eq!(reminder.tier, ReminderTier::Core);
    assert!(reminder.is_meta);
    assert_eq!(reminder.content(), Some("File foo.rs has been modified"));
    assert!(reminder.is_text());
}

#[test]
fn test_system_reminder_text() {
    let reminder = SystemReminder::text(
        AttachmentType::ChangedFiles,
        "File foo.rs has been modified",
    );

    assert_eq!(reminder.attachment_type, AttachmentType::ChangedFiles);
    assert!(reminder.is_text());
    assert!(!reminder.is_messages());
    assert_eq!(reminder.content(), Some("File foo.rs has been modified"));
}

#[test]
fn test_system_reminder_messages() {
    let messages = vec![
        ReminderMessage::assistant(vec![ContentBlock::tool_use(
            "test-id",
            "Read",
            serde_json::json!({"file_path": "/test.rs"}),
        )]),
        ReminderMessage::user(vec![ContentBlock::tool_result(
            "test-id",
            "file content here",
        )]),
    ];
    let reminder = SystemReminder::messages(AttachmentType::AlreadyReadFile, messages);

    assert_eq!(reminder.attachment_type, AttachmentType::AlreadyReadFile);
    assert!(!reminder.is_text());
    assert!(reminder.is_messages());
    assert!(reminder.content().is_none());
    assert!(reminder.wrapped_content().is_none());

    let msgs = reminder.output.as_messages().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, MessageRole::Assistant);
    assert_eq!(msgs[1].role, MessageRole::User);
}

#[test]
fn test_content_block_creation() {
    let text = ContentBlock::text("hello");
    assert!(matches!(text, ContentBlock::Text { text } if text == "hello"));

    let tool_use = ContentBlock::tool_use("id-1", "Read", serde_json::json!({}));
    assert!(
        matches!(tool_use, ContentBlock::ToolUse { id, name, .. } if id == "id-1" && name == "Read")
    );

    let tool_result = ContentBlock::tool_result("id-1", "result");
    assert!(
        matches!(tool_result, ContentBlock::ToolResult { tool_use_id, content } if tool_use_id == "id-1" && content == "result")
    );
}

#[test]
fn test_attachment_type_display() {
    assert_eq!(format!("{}", AttachmentType::ChangedFiles), "changed_files");
    assert_eq!(
        format!("{}", AttachmentType::PlanModeEnter),
        "plan_mode_enter"
    );
}

#[test]
fn test_already_read_file_type() {
    assert_eq!(
        AttachmentType::AlreadyReadFile.tier(),
        ReminderTier::MainAgentOnly
    );
    assert_eq!(AttachmentType::AlreadyReadFile.xml_tag(), XmlTag::None);
    assert_eq!(AttachmentType::AlreadyReadFile.name(), "already_read_file");
}