use super::*;
use crate::generator::TokenUsageStats;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_usage_returns_none() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = OutputTokenUsageGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_with_output_tokens_generates_content() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .token_usage(TokenUsageStats {
            input_tokens: 5000,
            output_tokens: 2000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            total_session_tokens: 50000,
            context_capacity: 200000,
            context_usage_percent: 25.0,
        })
        .build();

    let generator = OutputTokenUsageGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().expect("text content");
    assert!(content.contains("2.0K"));
}

#[tokio::test]
async fn test_disabled_returns_none() {
    let mut config = test_config();
    config.attachments.output_token_usage = false;

    let generator = OutputTokenUsageGenerator;
    assert!(!generator.is_enabled(&config));
}
