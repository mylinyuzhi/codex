use super::*;
use cocode_protocol::ToolName;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_tasks() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = UnifiedTasksGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_with_running_tasks() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .background_tasks(vec![BackgroundTaskInfo {
            task_id: "task-1".to_string(),
            task_type: BackgroundTaskType::Shell,
            command: "npm test".to_string(),
            status: BackgroundTaskStatus::Running,
            exit_code: None,
            has_new_output: true,
            progress_message: None,
            is_completion_notification: false,
            delta_summary: None,
            description: None,
        }])
        .build();

    let generator = UnifiedTasksGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Background Tasks"));
    assert!(reminder.content().unwrap().contains("Running"));
    assert!(reminder.content().unwrap().contains("npm test"));
    assert!(reminder.content().unwrap().contains("(new output)"));
    assert!(
        reminder
            .content()
            .unwrap()
            .contains(ToolName::TaskOutput.as_str())
    );
}

#[tokio::test]
async fn test_with_completed_tasks() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .background_tasks(vec![BackgroundTaskInfo {
            task_id: "task-2".to_string(),
            task_type: BackgroundTaskType::AsyncAgent,
            command: "explore codebase".to_string(),
            status: BackgroundTaskStatus::Completed,
            exit_code: Some(0),
            has_new_output: false,
            progress_message: None,
            is_completion_notification: false,
            delta_summary: None,
            description: None,
        }])
        .build();

    let generator = UnifiedTasksGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Completed"));
    assert!(reminder.content().unwrap().contains("[exit: 0]"));
    assert!(reminder.content().unwrap().contains("[agent]"));
}

#[tokio::test]
async fn test_with_failed_tasks() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .background_tasks(vec![BackgroundTaskInfo {
            task_id: "task-3".to_string(),
            task_type: BackgroundTaskType::Shell,
            command: "cargo build".to_string(),
            status: BackgroundTaskStatus::Failed,
            exit_code: Some(1),
            has_new_output: false,
            progress_message: None,
            is_completion_notification: false,
            delta_summary: None,
            description: None,
        }])
        .build();

    let generator = UnifiedTasksGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Failed"));
    assert!(reminder.content().unwrap().contains("[exit: 1]"));
}

#[tokio::test]
async fn test_mixed_tasks() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .background_tasks(vec![
            BackgroundTaskInfo {
                task_id: "t1".to_string(),
                task_type: BackgroundTaskType::Shell,
                command: "running cmd".to_string(),
                status: BackgroundTaskStatus::Running,
                exit_code: None,
                has_new_output: false,
                progress_message: None,
                is_completion_notification: false,
                delta_summary: None,
                description: None,
            },
            BackgroundTaskInfo {
                task_id: "t2".to_string(),
                task_type: BackgroundTaskType::AsyncAgent,
                command: "done cmd".to_string(),
                status: BackgroundTaskStatus::Completed,
                exit_code: Some(0),
                has_new_output: false,
                progress_message: None,
                is_completion_notification: false,
                delta_summary: None,
                description: None,
            },
            BackgroundTaskInfo {
                task_id: "t3".to_string(),
                task_type: BackgroundTaskType::RemoteSession,
                command: "remote session".to_string(),
                status: BackgroundTaskStatus::Failed,
                exit_code: Some(1),
                has_new_output: false,
                progress_message: None,
                is_completion_notification: false,
                delta_summary: None,
                description: None,
            },
        ])
        .build();

    let generator = UnifiedTasksGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("1 running, 1 completed, 1 failed")
    );
    assert!(reminder.content().unwrap().contains("[remote]"));
}

#[test]
fn test_generator_properties() {
    let generator = UnifiedTasksGenerator;
    assert_eq!(generator.name(), "UnifiedTasksGenerator");
    assert_eq!(generator.attachment_type(), AttachmentType::BackgroundTask);
    assert_eq!(generator.tier(), ReminderTier::MainAgentOnly);

    let config = test_config();
    assert!(generator.is_enabled(&config));

    // Throttled to every 3 turns (matches CC's TURNS_BETWEEN_PROGRESS)
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 3);
}

#[tokio::test]
async fn test_completion_notification_sets_bypass_throttle() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .background_tasks(vec![BackgroundTaskInfo {
            task_id: "t-done".to_string(),
            task_type: BackgroundTaskType::AsyncAgent,
            command: "explore".to_string(),
            status: BackgroundTaskStatus::Completed,
            exit_code: Some(0),
            has_new_output: false,
            progress_message: None,
            is_completion_notification: true,
            delta_summary: Some("Found 3 files".to_string()),
            description: None,
        }])
        .build();

    let generator = UnifiedTasksGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    assert!(
        reminder.bypass_throttle,
        "Completion notifications should bypass throttle"
    );
}

#[tokio::test]
async fn test_no_completion_notification_no_bypass() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .background_tasks(vec![BackgroundTaskInfo {
            task_id: "t-run".to_string(),
            task_type: BackgroundTaskType::Shell,
            command: "npm test".to_string(),
            status: BackgroundTaskStatus::Running,
            exit_code: None,
            has_new_output: false,
            progress_message: Some("50% done".to_string()),
            is_completion_notification: false,
            delta_summary: None,
            description: None,
        }])
        .build();

    let generator = UnifiedTasksGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    assert!(
        !reminder.bypass_throttle,
        "Running tasks should NOT bypass throttle"
    );
}

#[tokio::test]
async fn test_progress_message_in_output() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .background_tasks(vec![BackgroundTaskInfo {
            task_id: "t-prog".to_string(),
            task_type: BackgroundTaskType::AsyncAgent,
            command: "explore code".to_string(),
            status: BackgroundTaskStatus::Running,
            exit_code: None,
            has_new_output: false,
            progress_message: Some("Scanning 42 files".to_string()),
            is_completion_notification: false,
            delta_summary: None,
            description: None,
        }])
        .build();

    let generator = UnifiedTasksGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    assert!(
        reminder.content().unwrap().contains("Scanning 42 files"),
        "Should include progress message"
    );
}

#[tokio::test]
async fn test_delta_summary_in_output() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .background_tasks(vec![BackgroundTaskInfo {
            task_id: "t-delta".to_string(),
            task_type: BackgroundTaskType::AsyncAgent,
            command: "build project".to_string(),
            status: BackgroundTaskStatus::Completed,
            exit_code: Some(0),
            has_new_output: false,
            progress_message: None,
            is_completion_notification: true,
            delta_summary: Some("Build succeeded with 0 warnings".to_string()),
            description: None,
        }])
        .build();

    let generator = UnifiedTasksGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    let reminder = result.expect("reminder");
    assert!(
        reminder
            .content()
            .unwrap()
            .contains("Build succeeded with 0 warnings"),
        "Should include delta summary"
    );
}
