use super::*;
use crate::config::SystemReminderConfig;
use crate::types::ReminderTier;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_large_files() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        // No extension data
        .build();

    let generator = CompactFileReferenceGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_empty_large_files() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .extension(COMPACTED_LARGE_FILES_KEY, Vec::<CompactedLargeFile>::new())
        .build();

    let generator = CompactFileReferenceGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_generates_reference() {
    let config = test_config();
    let large_files = vec![
        CompactedLargeFile {
            path: PathBuf::from("/project/large.rs"),
            line_count: 5000,
            byte_size: 150000,
        },
        CompactedLargeFile {
            path: PathBuf::from("/project/huge.rs"),
            line_count: 10000,
            byte_size: 300000,
        },
    ];

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .extension(COMPACTED_LARGE_FILES_KEY, large_files)
        .build();

    let generator = CompactFileReferenceGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert_eq!(
        reminder.attachment_type,
        AttachmentType::CompactFileReference
    );
    assert!(reminder.is_text());

    let content = reminder.content().unwrap();
    assert!(content.contains("too large to include"));
    assert!(content.contains("/project/large.rs"));
    assert!(content.contains("5000 lines"));
    assert!(content.contains("/project/huge.rs"));
    assert!(content.contains("10000 lines"));
}

#[test]
fn test_generator_properties() {
    let generator = CompactFileReferenceGenerator;
    assert_eq!(generator.name(), "CompactFileReferenceGenerator");
    assert_eq!(
        generator.attachment_type(),
        AttachmentType::CompactFileReference
    );
    assert_eq!(generator.tier(), ReminderTier::MainAgentOnly);

    // No throttle for compact file references
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 0);
}

#[test]
fn test_is_enabled() {
    let mut config = test_config();
    let generator = CompactFileReferenceGenerator;

    assert!(generator.is_enabled(&config));

    config.attachments.compact_file_reference = false;
    assert!(!generator.is_enabled(&config));
}