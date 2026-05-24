use pretty_assertions::assert_eq;
use std::path::PathBuf;

use super::*;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_violations_returns_none() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = SandboxViolationsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_single_violation_with_path() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(3)
        .cwd(PathBuf::from("/tmp"))
        .sandbox_violations(vec![(
            "file-write-data".to_string(),
            Some("/etc/passwd".to_string()),
            None,
        )])
        .build();

    let generator = SandboxViolationsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().expect("text content");
    assert!(content.contains("<sandbox_violations>"));
    assert!(content.contains("1 violation(s) detected:"));
    assert!(content.contains("file-write-data"));
    assert!(content.contains("path=/etc/passwd"));
    assert!(content.contains("</sandbox_violations>"));
}

#[tokio::test]
async fn test_violation_with_command_tag() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .sandbox_violations(vec![(
            "network-outbound".to_string(),
            None,
            Some("cmd_abc123".to_string()),
        )])
        .build();

    let generator = SandboxViolationsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().expect("text content");
    assert!(content.contains("network-outbound"));
    assert!(content.contains("cmd=cmd_abc123"));
    assert!(!content.contains("path="));
}

#[tokio::test]
async fn test_multiple_violations() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(7)
        .cwd(PathBuf::from("/tmp"))
        .sandbox_violations(vec![
            (
                "file-write-data".to_string(),
                Some("/etc/hosts".to_string()),
                Some("cmd_1".to_string()),
            ),
            ("network-outbound".to_string(), None, None),
            (
                "file-read-data".to_string(),
                Some("/root/.ssh/id_rsa".to_string()),
                Some("cmd_2".to_string()),
            ),
        ])
        .build();

    let generator = SandboxViolationsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().expect("text content");
    assert!(content.contains("3 violation(s) detected:"));
    assert!(content.contains("file-write-data"));
    assert!(content.contains("network-outbound"));
    assert!(content.contains("file-read-data"));
    assert!(content.contains("/root/.ssh/id_rsa"));
}

#[tokio::test]
async fn test_attachment_type() {
    let generator = SandboxViolationsGenerator;
    assert_eq!(
        generator.attachment_type(),
        AttachmentType::SandboxViolations
    );
}

#[test]
fn test_throttle_config_is_none() {
    let generator = SandboxViolationsGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 0);
}

#[test]
fn test_disabled_returns_false() {
    let mut config = test_config();
    config.attachments.sandbox_violations = false;

    let generator = SandboxViolationsGenerator;
    assert!(!generator.is_enabled(&config));
}

#[test]
fn test_enabled_by_default() {
    let config = test_config();
    let generator = SandboxViolationsGenerator;
    assert!(generator.is_enabled(&config));
}
