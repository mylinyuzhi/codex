use super::*;
use crate::generator::StructuredTaskInfo;
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

// ── Structured task rendering ────────────────────────────────

#[tokio::test]
async fn test_structured_tasks_rich_format() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .structured_tasks(vec![
            StructuredTaskInfo {
                id: "task_abc".to_string(),
                subject: "Implement login UI".to_string(),
                description: Some("Build the login form component".to_string()),
                status: "in_progress".to_string(),
                active_form: Some("Implementing login UI".to_string()),
                owner: Some("agent".to_string()),
                blocks: vec!["task_def".to_string()],
                blocked_by: Vec::new(),
                is_blocked: false,
            },
            StructuredTaskInfo {
                id: "task_def".to_string(),
                subject: "Write tests".to_string(),
                description: None,
                status: "pending".to_string(),
                active_form: None,
                owner: None,
                blocks: Vec::new(),
                blocked_by: vec!["task_abc".to_string()],
                is_blocked: true,
            },
            StructuredTaskInfo {
                id: "task_ghi".to_string(),
                subject: "Setup CI".to_string(),
                description: None,
                status: "completed".to_string(),
                active_form: None,
                owner: None,
                blocks: Vec::new(),
                blocked_by: Vec::new(),
                is_blocked: false,
            },
        ])
        .build();

    let generator = TodoRemindersGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let text = reminder.content().unwrap();

    // In Progress section with rich info
    assert!(text.contains("In Progress"), "got: {text}");
    assert!(
        text.contains("[>] task_abc: Implement login UI"),
        "got: {text}"
    );
    assert!(text.contains("Owner: agent"), "got: {text}");
    assert!(text.contains("Blocks: task_def"), "got: {text}");
    assert!(
        text.contains("Description: Build the login form component"),
        "got: {text}"
    );

    // Pending section with blocker info
    assert!(text.contains("Pending"), "got: {text}");
    assert!(text.contains("[ ] task_def: Write tests"), "got: {text}");
    assert!(text.contains("Blocked by: task_abc"), "got: {text}");

    // Summary includes completed count
    assert!(text.contains("1/3 tasks completed"), "got: {text}");
}

#[tokio::test]
async fn test_structured_tasks_preferred_over_plain() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        // Both plain and structured present — structured wins
        .todos(vec![TodoItem {
            id: "plain-1".to_string(),
            subject: "Plain task".to_string(),
            status: TodoStatus::Pending,
            is_blocked: false,
        }])
        .structured_tasks(vec![StructuredTaskInfo {
            id: "struct-1".to_string(),
            subject: "Structured task".to_string(),
            description: None,
            status: "pending".to_string(),
            active_form: None,
            owner: None,
            blocks: Vec::new(),
            blocked_by: Vec::new(),
            is_blocked: false,
        }])
        .build();

    let generator = TodoRemindersGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let text = result.expect("reminder").content().unwrap().to_string();

    // Should use structured format, not plain
    assert!(text.contains("struct-1"), "got: {text}");
    assert!(!text.contains("plain-1"), "got: {text}");
}
