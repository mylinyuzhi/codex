use super::*;
use std::collections::HashSet;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_triggers() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = NestedMemoryGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_with_nonexistent_trigger() {
    let config = test_config();
    let mut triggers = HashSet::new();
    triggers.insert(PathBuf::from("/nonexistent/CLAUDE.md"));

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .nested_memory_triggers(triggers)
        .build();

    let generator = NestedMemoryGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    // File doesn't exist, so no content
    assert!(result.is_none());
}

#[test]
fn test_truncate_content_by_lines() {
    let content = "line1\nline2\nline3\nline4\nline5";
    let truncated = truncate_content(content, 10000, 3);

    assert!(truncated.contains("line1"));
    assert!(truncated.contains("line2"));
    assert!(truncated.contains("line3"));
    assert!(!truncated.contains("line4"));
    assert!(truncated.contains("truncated"));
}

#[test]
fn test_truncate_content_by_bytes() {
    let content = "This is a very long line that should be truncated";
    let truncated = truncate_content(content, 20, 1000);

    assert!(truncated.len() <= 60); // Some overhead for truncation message
    assert!(truncated.contains("truncated") || truncated.len() <= 20);
}

#[test]
fn test_truncate_content_fits() {
    let content = "short";
    let truncated = truncate_content(content, 10000, 1000);

    assert_eq!(truncated, "short");
    assert!(!truncated.contains("truncated"));
}

#[test]
fn test_generator_properties() {
    let generator = NestedMemoryGenerator;
    assert_eq!(generator.name(), "NestedMemoryGenerator");
    assert_eq!(generator.attachment_type(), AttachmentType::NestedMemory);

    let config = test_config();
    assert!(generator.is_enabled(&config));
}

#[test]
fn test_disabled_in_config() {
    let mut config = test_config();
    config.nested_memory.enabled = false;

    let generator = NestedMemoryGenerator;
    assert!(!generator.is_enabled(&config));
}
