use super::*;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_name_returns_none() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = SessionNameGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_with_name_generates_content() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .session_name("my-refactor")
        .build();

    let generator = SessionNameGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().expect("text content");
    assert!(content.contains("my-refactor"));
}

#[tokio::test]
async fn test_disabled_returns_none() {
    let mut config = test_config();
    config.attachments.session_name = false;

    let generator = SessionNameGenerator;
    assert!(!generator.is_enabled(&config));
}
