use super::*;
use crate::generator::ApprovedPlanInfo;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_plan_mode_enter_not_in_plan_mode() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(false)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_plan_mode_enter_full() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(false)
        .plan_file_path(PathBuf::from("/home/user/.cocode/plans/test-plan.md"))
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Phase 1: Understand"));
    assert!(reminder.content().unwrap().contains("Phase 5: Review"));
    assert!(reminder.content().unwrap().contains("Write tool"));
    assert!(reminder.content().unwrap().contains("Edit tool"));
    assert!(
        reminder
            .content()
            .unwrap()
            .contains(".cocode/plans/test-plan.md")
    );
}

#[tokio::test]
async fn test_plan_mode_enter_sparse_via_reentry() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(true)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(!reminder.content().unwrap().contains("Phase 1")); // Sparse doesn't have phases
    assert!(reminder.content().unwrap().contains("ExitPlanMode"));
}

#[tokio::test]
async fn test_plan_mode_enter_sparse_via_turn() {
    let config = test_config();
    // Turn 2 should use sparse reminders (not turn 1 or turn % 5 == 1)
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(2)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(false)
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(!reminder.content().unwrap().contains("Phase 1")); // Sparse doesn't have phases
    assert!(reminder.content().unwrap().contains("ExitPlanMode"));
}

#[tokio::test]
async fn test_plan_mode_enter_full_on_turn_6() {
    let config = test_config();
    // Turn 6 (5+1) should use full reminders
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(6)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .is_plan_reentry(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .build();

    let generator = PlanModeEnterGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Phase 1")); // Full has phases
    assert!(reminder.content().unwrap().contains("Phase 5"));
}

#[tokio::test]
async fn test_plan_mode_approved() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .approved_plan(ApprovedPlanInfo {
            content: "Step 1: Do something\nStep 2: Do more".to_string(),
            approved_turn: 5,
        })
        .build();

    let generator = PlanModeApprovedGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Approved Plan"));
    assert!(reminder.content().unwrap().contains("Step 1: Do something"));
}

#[tokio::test]
async fn test_plan_tool_reminder() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(10)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .plan_file_path(PathBuf::from("/home/user/.cocode/plans/plan.md"))
        .build();

    let generator = PlanToolReminderGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Write tool"));
    assert!(reminder.content().unwrap().contains("Edit tool"));
    assert!(
        reminder
            .content()
            .unwrap()
            .contains(".cocode/plans/plan.md")
    );
}

#[tokio::test]
async fn test_plan_tool_reminder_no_plan_path() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(10)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        // No plan_file_path
        .build();

    let generator = PlanToolReminderGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[test]
fn test_throttle_configs() {
    let enter_generator = PlanModeEnterGenerator;
    let throttle = enter_generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 5);

    let tool_generator = PlanToolReminderGenerator;
    let throttle = tool_generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 3);
    assert_eq!(throttle.min_turns_after_trigger, 5);
}
