use super::*;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_worktrees_returns_none() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = WorktreeStateGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_active_worktrees_generates_content() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .active_worktree_count(3)
        .build();

    let generator = WorktreeStateGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().expect("text content");
    assert!(content.contains("3"));
    assert!(content.contains("worktree"));
}

#[tokio::test]
async fn test_disabled_returns_none() {
    let mut config = test_config();
    config.attachments.worktree_state = false;

    let generator = WorktreeStateGenerator;
    assert!(!generator.is_enabled(&config));
}
