use std::collections::HashMap;

use super::*;
use crate::config::SystemReminderConfig;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_auto_mode_enter_not_active() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .is_auto_mode(false)
        .build();
    let generator = AutoModeEnterGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_auto_mode_enter_full() {
    let config = test_config();
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::AutoMode, true);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .is_auto_mode(true)
        .build();
    ctx.full_content_flags = flags;
    let generator = AutoModeEnterGenerator;
    let reminder = generator.generate(&ctx).await.unwrap().unwrap();
    let content = reminder.content().unwrap();
    assert!(content.contains("Auto Mode Active"));
    assert!(content.contains("Execute immediately"));
    assert!(content.contains("Be thorough"));
}

#[tokio::test]
async fn test_auto_mode_enter_sparse() {
    let config = test_config();
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::AutoMode, false);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .is_auto_mode(true)
        .build();
    ctx.full_content_flags = flags;
    let generator = AutoModeEnterGenerator;
    let reminder = generator.generate(&ctx).await.unwrap().unwrap();
    let content = reminder.content().unwrap();
    assert!(content.contains("Auto mode still active"));
}

#[tokio::test]
async fn test_auto_mode_exit_not_pending() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .auto_mode_exit_pending(false)
        .build();
    let generator = AutoModeExitGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_auto_mode_exit_pending() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .auto_mode_exit_pending(true)
        .build();
    let generator = AutoModeExitGenerator;
    let reminder = generator.generate(&ctx).await.unwrap().unwrap();
    let content = reminder.content().unwrap();
    assert!(content.contains("Exited Auto Mode"));
    assert!(content.contains("ask clarifying questions"));
}

#[test]
fn test_throttle_configs() {
    let enter = AutoModeEnterGenerator;
    let throttle = enter.throttle_config();
    assert_eq!(throttle.min_turns_between, 5);

    let exit = AutoModeExitGenerator;
    let throttle = exit.throttle_config();
    assert_eq!(throttle.min_turns_between, 0);
}
