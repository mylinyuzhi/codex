use super::*;
use crate::generator::PlanState;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_not_triggered_in_plan_mode() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .plan_state(PlanState {
            is_empty: false,
            last_update_turn: 1,
            steps: vec![PlanStep {
                step: "Step 1".to_string(),
                status: "pending".to_string(),
            }],
        })
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_not_triggered_without_plan() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(false)
        // No plan_state
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_not_triggered_for_empty_plan() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(false)
        .plan_state(PlanState {
            is_empty: true,
            last_update_turn: 1,
            steps: vec![],
        })
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_shows_progress() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(10)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(false)
        .plan_state(PlanState {
            is_empty: false,
            last_update_turn: 5,
            steps: vec![
                PlanStep {
                    step: "Set up database".to_string(),
                    status: "completed".to_string(),
                },
                PlanStep {
                    step: "Create API endpoints".to_string(),
                    status: "in_progress".to_string(),
                },
                PlanStep {
                    step: "Add authentication".to_string(),
                    status: "pending".to_string(),
                },
                PlanStep {
                    step: "Write tests".to_string(),
                    status: "pending".to_string(),
                },
            ],
        })
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Plan Progress: 1/4"));
    assert!(reminder.content().unwrap().contains("Current:"));
    assert!(reminder.content().unwrap().contains("Create API endpoints"));
    assert!(reminder.content().unwrap().contains("Next:"));
    assert!(reminder.content().unwrap().contains("Add authentication"));
}

#[test]
fn test_throttle_config() {
    let generator = PlanVerificationGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 5);
    assert_eq!(throttle.min_turns_after_trigger, 3);
}
