use super::*;
use crate::generator::GeneratorContext;
use crate::generator::TaskRunStatus;
use crate::generator::TaskStatusSnapshot;
use coco_config::SystemReminderConfig;

fn snap(id: &str, desc: &str, status: TaskRunStatus) -> TaskStatusSnapshot {
    TaskStatusSnapshot {
        task_id: id.into(),
        description: desc.into(),
        status,
        delta_summary: None,
        output_file_path: None,
    }
}

#[tokio::test]
async fn skips_when_no_statuses() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c).task_statuses(vec![]).build();
    assert!(TaskStatusGenerator.generate(&ctx).await.unwrap().is_none());
}

#[tokio::test]
async fn killed_renders_stopped_message() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .task_statuses(vec![snap("42", "code review", TaskRunStatus::Killed)])
        .build();
    let text = TaskStatusGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert_eq!(text, "Task \"code review\" (42) was stopped by the user.");
}

#[tokio::test]
async fn running_includes_anti_duplicate_warning() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .task_statuses(vec![TaskStatusSnapshot {
            task_id: "42".into(),
            description: "scan repo".into(),
            status: TaskRunStatus::Running,
            delta_summary: Some("10/100 files".into()),
            output_file_path: Some("/tmp/task-42.log".into()),
        }])
        .build();
    let text = TaskStatusGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert!(text.contains("Background agent \"scan repo\" (42) is still running."));
    assert!(text.contains("Progress: 10/100 files"));
    assert!(text.contains("Do NOT spawn a duplicate"));
    assert!(text.contains("/tmp/task-42.log"));
}

#[tokio::test]
async fn completed_includes_delta_when_set() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .task_statuses(vec![TaskStatusSnapshot {
            task_id: "x".into(),
            description: "tidy".into(),
            status: TaskRunStatus::Completed,
            delta_summary: Some("removed 3 files".into()),
            output_file_path: None,
        }])
        .build();
    let text = TaskStatusGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert!(text.starts_with("Task \"tidy\" (x) completed."));
    assert!(text.contains("Result: removed 3 files"));
}
