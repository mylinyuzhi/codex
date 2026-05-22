use super::*;
use crate::generator::CollabNotification;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_not_triggered_without_notifications() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        // No collab_notifications
        .build();

    let generator = CollabNotificationsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_shows_error_notifications() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .cwd(PathBuf::from("/tmp"))
        .collab_notifications(vec![CollabNotification {
            from_agent: "explore-agent".to_string(),
            notification_type: "error".to_string(),
            message: "Failed to read file: permission denied".to_string(),
            received_turn: 4,
        }])
        .build();

    let generator = CollabNotificationsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Agent Notifications"));
    assert!(reminder.content().unwrap().contains("Errors"));
    assert!(reminder.content().unwrap().contains("explore-agent"));
    assert!(reminder.content().unwrap().contains("permission denied"));
}

#[tokio::test]
async fn test_shows_mixed_notifications() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(10)
        .cwd(PathBuf::from("/tmp"))
        .collab_notifications(vec![
            CollabNotification {
                from_agent: "search-agent".to_string(),
                notification_type: "completed".to_string(),
                message: "Found 5 matching files".to_string(),
                received_turn: 8,
            },
            CollabNotification {
                from_agent: "plan-agent".to_string(),
                notification_type: "needs_input".to_string(),
                message: "Need clarification on database choice".to_string(),
                received_turn: 9,
            },
        ])
        .build();

    let generator = CollabNotificationsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Awaiting Input"));
    assert!(reminder.content().unwrap().contains("Completed"));
    assert!(reminder.content().unwrap().contains("search-agent"));
    assert!(reminder.content().unwrap().contains("plan-agent"));
}

#[test]
fn test_throttle_config() {
    let generator = CollabNotificationsGenerator;
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 0);
}
