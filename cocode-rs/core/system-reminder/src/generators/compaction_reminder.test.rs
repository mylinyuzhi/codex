use super::*;
use crate::config::SystemReminderConfig;
use crate::generator::GeneratorContext;

#[tokio::test]
async fn test_generates_when_auto_compact_enabled() {
    let config = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .is_auto_compact_enabled(true)
        .build();

    let generator = CompactionReminderGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_some());
    let reminder = result.unwrap();
    let text = reminder.output.as_text().unwrap();
    assert!(text.contains("Auto-compact is enabled"));
    assert!(text.contains("unlimited context"));
}

#[tokio::test]
async fn test_skips_when_auto_compact_disabled() {
    let config = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .cwd(std::path::PathBuf::from("/tmp"))
        .is_auto_compact_enabled(false)
        .build();

    let generator = CompactionReminderGenerator;
    let result = generator.generate(&ctx).await.unwrap();
    assert!(result.is_none());
}
