use super::*;
use crate::generator::GeneratorContext;
use crate::generator::QueuedCommandInfo;
use coco_config::SystemReminderConfig;

#[tokio::test]
async fn skips_when_empty() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .queued_commands(vec![])
        .build();
    assert!(
        QueuedCommandGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn skips_human_origin_commands() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .queued_commands(vec![QueuedCommandInfo {
            content: "human typed this".into(),
            origin_system: false,
        }])
        .build();
    assert!(
        QueuedCommandGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn emits_system_origin_joined_by_blank_line() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .queued_commands(vec![
            QueuedCommandInfo {
                content: "Task A completed".into(),
                origin_system: true,
            },
            QueuedCommandInfo {
                content: "Task B failed".into(),
                origin_system: true,
            },
        ])
        .build();
    let text = QueuedCommandGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert!(text.contains("Task A completed"));
    assert!(text.contains("Task B failed"));
    assert!(text.contains("Task A completed\n\nTask B failed"));
}
