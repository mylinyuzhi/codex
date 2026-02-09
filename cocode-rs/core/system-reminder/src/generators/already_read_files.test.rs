use super::*;
use crate::config::SystemReminderConfig;
use crate::file_tracker::FileTracker;
use crate::file_tracker::ReadFileState;
use crate::types::ReminderTier;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_tracker() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = AlreadyReadFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_empty_tracker() {
    let config = test_config();
    let tracker = FileTracker::new();

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .file_tracker(&tracker)
        .build();

    let generator = AlreadyReadFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_generates_tool_pairs() {
    let config = test_config();
    let tracker = FileTracker::new();

    // Track a file read
    let state = ReadFileState::new("fn main() {}\n".to_string(), None, 1);
    tracker.track_read("/project/src/main.rs", state);

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .file_tracker(&tracker)
        .build();

    let generator = AlreadyReadFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert_eq!(reminder.attachment_type, AttachmentType::AlreadyReadFile);
    assert!(reminder.is_messages());

    let messages = reminder.output.as_messages().unwrap();
    assert_eq!(messages.len(), 2); // One tool_use + one tool_result

    // Check assistant message with tool_use
    assert_eq!(messages[0].role, MessageRole::Assistant);
    assert!(matches!(
        &messages[0].blocks[0],
        ContentBlock::ToolUse { name, .. } if name == "Read"
    ));

    // Check user message with tool_result
    assert_eq!(messages[1].role, MessageRole::User);
    assert!(matches!(
        &messages[1].blocks[0],
        ContentBlock::ToolResult { content, .. } if content.contains("Previously read")
    ));
}

#[tokio::test]
async fn test_partial_read_summary() {
    let config = test_config();
    let tracker = FileTracker::new();

    // Track a partial file read
    let state = ReadFileState::partial("partial content".to_string(), None, 1, 10, 50);
    tracker.track_read("/project/large.rs", state);

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .file_tracker(&tracker)
        .build();

    let generator = AlreadyReadFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let messages = reminder.output.as_messages().unwrap();

    // Check that partial read is indicated
    if let ContentBlock::ToolResult { content, .. } = &messages[1].blocks[0] {
        assert!(content.contains("partial"));
    } else {
        panic!("Expected ToolResult");
    }
}

#[tokio::test]
async fn test_multiple_files() {
    let config = test_config();
    let tracker = FileTracker::new();

    // Track multiple files
    tracker.track_read(
        "/project/a.rs",
        ReadFileState::new("a".to_string(), None, 1),
    );
    tracker.track_read(
        "/project/b.rs",
        ReadFileState::new("b".to_string(), None, 1),
    );
    tracker.track_read(
        "/project/c.rs",
        ReadFileState::new("c".to_string(), None, 1),
    );

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .file_tracker(&tracker)
        .build();

    let generator = AlreadyReadFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let messages = reminder.output.as_messages().unwrap();
    // 3 files × 2 messages each = 6 messages
    assert_eq!(messages.len(), 6);
}

#[test]
fn test_generator_properties() {
    let generator = AlreadyReadFilesGenerator;
    assert_eq!(generator.name(), "AlreadyReadFilesGenerator");
    assert_eq!(generator.attachment_type(), AttachmentType::AlreadyReadFile);
    assert_eq!(generator.tier(), ReminderTier::MainAgentOnly);

    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 5);
}