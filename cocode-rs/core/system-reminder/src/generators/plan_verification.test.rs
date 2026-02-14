use std::path::PathBuf;

use super::*;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

fn completed_todo(id: &str) -> TodoItem {
    TodoItem {
        id: id.to_string(),
        subject: format!("Task {id}"),
        status: TodoStatus::Completed,
        is_blocked: false,
    }
}

fn pending_todo(id: &str) -> TodoItem {
    TodoItem {
        id: id.to_string(),
        subject: format!("Task {id}"),
        status: TodoStatus::Pending,
        is_blocked: false,
    }
}

/// Helper: build a context that satisfies all trigger conditions.
fn firing_context(config: &SystemReminderConfig) -> GeneratorContext<'_> {
    GeneratorContext::builder()
        .config(config)
        .turn_number(10)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .todos(vec![completed_todo("1"), completed_todo("2")])
        .build()
}

use crate::generator::TodoItem;

#[tokio::test]
async fn test_returns_none_in_plan_mode() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(true)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .todos(vec![completed_todo("1")])
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_returns_none_for_subagent() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(false)
        .is_plan_mode(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .todos(vec![completed_todo("1")])
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_returns_none_without_plan_file() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(false)
        .todos(vec![completed_todo("1")])
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_returns_none_without_todos() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_returns_none_with_pending_todos() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .is_main_agent(true)
        .is_plan_mode(false)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .todos(vec![pending_todo("1"), completed_todo("2")])
        .build();

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_fires_when_all_todos_completed() {
    let config = test_config();
    let ctx = firing_context(&config);

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.unwrap();
    assert_eq!(reminder.attachment_type, AttachmentType::PlanVerification);
}

#[tokio::test]
async fn test_content_matches_cc_template() {
    let config = test_config();
    let ctx = firing_context(&config);

    let generator = PlanVerificationGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("should fire");

    let expected = "You have completed implementing the plan. \
                    Please call the \"\" tool directly \
                    (NOT the Task tool or an agent) to verify \
                    that all plan items were completed correctly.";
    assert_eq!(reminder.content(), Some(expected));
}

#[test]
fn test_attachment_type() {
    let generator = PlanVerificationGenerator;
    assert_eq!(
        generator.attachment_type(),
        AttachmentType::PlanVerification
    );
}

#[test]
fn test_throttle_config_min_turns_between() {
    let generator = PlanVerificationGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 5);
}
