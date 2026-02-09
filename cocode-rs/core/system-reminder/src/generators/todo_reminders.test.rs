use super::*;
use crate::generator::TodoItem;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_todos() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = TodoRemindersGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_with_todos() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .todos(vec![
            TodoItem {
                id: "1".to_string(),
                subject: "Implement feature X".to_string(),
                status: TodoStatus::InProgress,
                is_blocked: false,
            },
            TodoItem {
                id: "2".to_string(),
                subject: "Write tests".to_string(),
                status: TodoStatus::Pending,
                is_blocked: false,
            },
            TodoItem {
                id: "3".to_string(),
                subject: "Update docs".to_string(),
                status: TodoStatus::Completed,
                is_blocked: false,
            },
        ])
        .build();

    let generator = TodoRemindersGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("In Progress"));
    assert!(reminder.content().unwrap().contains("Implement feature X"));
    assert!(reminder.content().unwrap().contains("Pending"));
    assert!(reminder.content().unwrap().contains("Write tests"));
    assert!(reminder.content().unwrap().contains("1/3 tasks completed"));
}

#[tokio::test]
async fn test_blocked_tasks() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .todos(vec![TodoItem {
            id: "1".to_string(),
            subject: "Blocked task".to_string(),
            status: TodoStatus::Pending,
            is_blocked: true,
        }])
        .build();

    let generator = TodoRemindersGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("(blocked)"));
}

#[test]
fn test_generator_properties() {
    let generator = TodoRemindersGenerator;
    assert_eq!(generator.name(), "TodoRemindersGenerator");
    assert_eq!(generator.tier(), ReminderTier::MainAgentOnly);

    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 5);
}
