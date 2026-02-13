use super::*;
use crate::config::SystemReminderConfig;
use crate::generator::BudgetInfo;
use crate::types::ReminderTier;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_budget() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        // No budget
        .build();

    let generator = BudgetUsdGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_budget_not_low() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .budget(BudgetInfo {
            total_usd: 10.0,
            used_usd: 5.0,
            remaining_usd: 5.0,
            is_low: false, // 50% remaining
        })
        .build();

    let generator = BudgetUsdGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_budget_low() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .budget(BudgetInfo {
            total_usd: 10.0,
            used_usd: 9.5,
            remaining_usd: 0.5,
            is_low: true, // 5% remaining
        })
        .build();

    let generator = BudgetUsdGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert_eq!(reminder.attachment_type, AttachmentType::BudgetUsd);
    assert!(reminder.is_text());
    assert!(reminder.content().unwrap().contains("Budget Warning"));
    assert!(reminder.content().unwrap().contains("$0.50"));
    assert!(reminder.content().unwrap().contains("95.0%"));
}

#[tokio::test]
async fn test_budget_below_threshold() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .budget(BudgetInfo {
            total_usd: 100.0,
            used_usd: 92.0,
            remaining_usd: 8.0,
            is_low: false, // 8% remaining, below 10% threshold
        })
        .build();

    let generator = BudgetUsdGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some()); // Should generate because below threshold

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("$8.00"));
}

#[test]
fn test_generator_properties() {
    let generator = BudgetUsdGenerator;
    assert_eq!(generator.name(), "BudgetUsdGenerator");
    assert_eq!(generator.attachment_type(), AttachmentType::BudgetUsd);
    assert_eq!(generator.tier(), ReminderTier::MainAgentOnly);

    // No throttle for budget warnings
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 0);
}
