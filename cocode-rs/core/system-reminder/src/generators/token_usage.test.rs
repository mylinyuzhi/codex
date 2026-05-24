use super::*;
use crate::generator::BudgetInfo;
use crate::generator::TokenUsageStats;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_not_triggered_without_usage() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        // No token_usage
        .build();

    let generator = TokenUsageGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_normal_usage() {
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

    let generator = TokenUsageGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Token Usage"));
    assert!(reminder.content().unwrap().contains("25.0%"));
    assert!(!reminder.content().unwrap().contains("Warning"));
}

#[tokio::test]
async fn test_high_usage_warning() {
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
            total_session_tokens: 170000,
            context_capacity: 200000,
            context_usage_percent: 85.0,
        })
        .build();

    let generator = TokenUsageGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Warning"));
    assert!(reminder.content().unwrap().contains("85.0%"));
}

#[tokio::test]
async fn test_critical_usage() {
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
            total_session_tokens: 195000,
            context_capacity: 200000,
            context_usage_percent: 97.5,
        })
        .build();

    let generator = TokenUsageGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("CRITICAL"));
    assert!(reminder.content().unwrap().contains("summarizing"));
}

#[tokio::test]
async fn test_with_budget_warning() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .token_usage(TokenUsageStats {
            context_usage_percent: 50.0,
            context_capacity: 200000,
            ..Default::default()
        })
        .budget(BudgetInfo {
            total_usd: 10.0,
            used_usd: 9.5,
            remaining_usd: 0.5,
            is_low: true,
        })
        .build();

    let generator = TokenUsageGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Budget Warning"));
    assert!(reminder.content().unwrap().contains("$0.50"));
}

#[test]
fn test_format_tokens() {
    assert_eq!(format_tokens(500), "500");
    assert_eq!(format_tokens(1500), "1.5K");
    assert_eq!(format_tokens(1_500_000), "1.5M");
}

#[test]
fn test_throttle_config() {
    let generator = TokenUsageGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 10);
}
