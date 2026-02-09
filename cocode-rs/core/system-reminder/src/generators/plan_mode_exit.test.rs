use super::*;
use crate::generator::ApprovedPlanInfo;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_not_triggered_without_pending_flag() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .plan_mode_exit_pending(false)
        .approved_plan(ApprovedPlanInfo {
            content: "Step 1: Do something".to_string(),
            approved_turn: 5,
        })
        .build();

    let generator = PlanModeExitGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_not_triggered_without_approved_plan() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .plan_mode_exit_pending(true)
        // No approved_plan
        .build();

    let generator = PlanModeExitGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_triggered_with_pending_and_approved() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .plan_mode_exit_pending(true)
        .approved_plan(ApprovedPlanInfo {
            content: "Step 1: Implement feature X\nStep 2: Add tests".to_string(),
            approved_turn: 5,
        })
        .build();

    let generator = PlanModeExitGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Plan Approved"));
    assert!(reminder.content().unwrap().contains("Begin Implementation"));
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Step 1: Implement feature X")
    );
}

#[test]
fn test_throttle_config() {
    let generator = PlanModeExitGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 0);
}
