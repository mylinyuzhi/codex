use super::*;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[test]
fn test_context_builder() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(5)
        .is_main_agent(true)
        .has_user_input(true)
        .cwd(PathBuf::from("/tmp/test"))
        .build();

    assert_eq!(ctx.turn_number, 5);
    assert!(ctx.is_main_agent);
    assert!(ctx.has_user_input);
    assert!(!ctx.in_plan_mode());
    assert!(!ctx.has_background_tasks());
}

#[test]
fn test_context_plan_mode() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .is_plan_mode(true)
        .plan_file_path(PathBuf::from("/tmp/plan.md"))
        .build();

    assert!(ctx.in_plan_mode());
    assert_eq!(ctx.plan_file_path, Some(PathBuf::from("/tmp/plan.md")));
}

#[test]
fn test_todo_filtering() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .todos(vec![
            TodoItem {
                id: "1".to_string(),
                subject: "Task 1".to_string(),
                status: TodoStatus::Pending,
                is_blocked: false,
            },
            TodoItem {
                id: "2".to_string(),
                subject: "Task 2".to_string(),
                status: TodoStatus::InProgress,
                is_blocked: false,
            },
            TodoItem {
                id: "3".to_string(),
                subject: "Task 3".to_string(),
                status: TodoStatus::Completed,
                is_blocked: false,
            },
        ])
        .build();

    assert!(ctx.has_todos());
    assert_eq!(ctx.pending_todos().count(), 1);
    assert_eq!(ctx.in_progress_todos().count(), 1);
}

#[test]
fn test_background_task_info() {
    let task = BackgroundTaskInfo {
        task_id: "task-1".to_string(),
        task_type: BackgroundTaskType::Shell,
        command: "npm test".to_string(),
        status: BackgroundTaskStatus::Running,
        exit_code: None,
        has_new_output: true,
    };

    assert_eq!(task.task_type, BackgroundTaskType::Shell);
    assert_eq!(task.status, BackgroundTaskStatus::Running);
    assert!(task.has_new_output);
}

#[test]
fn test_should_use_full_content() {
    let config = test_config();

    // Default (no flag set) → full
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();
    assert!(ctx.should_use_full_content(AttachmentType::SecurityGuidelines));

    // Explicit full flag
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::SecurityGuidelines, true);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();
    ctx.full_content_flags = flags;
    assert!(ctx.should_use_full_content(AttachmentType::SecurityGuidelines));

    // Explicit sparse flag
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::SecurityGuidelines, false);
    let mut ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(2)
        .cwd(PathBuf::from("/tmp"))
        .build();
    ctx.full_content_flags = flags;
    assert!(!ctx.should_use_full_content(AttachmentType::SecurityGuidelines));

    // Flag for one type doesn't affect another
    assert!(ctx.should_use_full_content(AttachmentType::PlanModeEnter));
}
