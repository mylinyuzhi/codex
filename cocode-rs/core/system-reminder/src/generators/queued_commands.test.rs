use super::*;
use crate::generator::QueuedCommandInfo;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_not_triggered_without_queued_commands() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = QueuedCommandsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_generates_user_sent_format() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .queued_commands(vec![QueuedCommandInfo {
            id: "cmd-1".to_string(),
            prompt: "use TypeScript instead".to_string(),
            queued_at: 1234567890,
        }])
        .build();

    let generator = QueuedCommandsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert_eq!(reminder.attachment_type, AttachmentType::QueuedCommands);
    assert_eq!(
        reminder.content().unwrap(),
        "The user sent the following message:\n\
         use TypeScript instead\n\n\
         Please address this message and continue with your tasks."
    );
    assert!(reminder.is_meta);
}

#[tokio::test]
async fn test_generates_multiple_commands() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(10)
        .cwd(PathBuf::from("/tmp"))
        .queued_commands(vec![
            QueuedCommandInfo {
                id: "cmd-1".to_string(),
                prompt: "use TypeScript".to_string(),
                queued_at: 1234567890,
            },
            QueuedCommandInfo {
                id: "cmd-2".to_string(),
                prompt: "add error handling".to_string(),
                queued_at: 1234567891,
            },
        ])
        .build();

    let generator = QueuedCommandsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    assert!(content.contains("The user sent the following message:\nuse TypeScript\n"));
    assert!(content.contains("The user sent the following message:\nadd error handling\n"));
    assert!(content.contains("Please address this message and continue with your tasks."));
}

#[test]
fn test_throttle_config() {
    let generator = QueuedCommandsGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 0);
}

#[test]
fn test_always_enabled() {
    let generator = QueuedCommandsGenerator;
    let config = test_config();
    assert!(generator.is_enabled(&config));
}
