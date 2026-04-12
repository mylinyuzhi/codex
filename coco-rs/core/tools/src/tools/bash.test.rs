use crate::tools::bash::BashTool;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;

#[tokio::test]
async fn test_bash_echo() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "echo hello world"}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("hello world"));
}

#[tokio::test]
async fn test_bash_exit_code() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "exit 42"}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("Exit code: 42"));
}

#[tokio::test]
async fn test_bash_stderr() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "echo err >&2"}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("err"));
}

#[tokio::test]
async fn test_bash_timeout() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "sleep 10", "timeout": 100}), &ctx)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("timed out"));
}

#[tokio::test]
async fn test_bash_pwd() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "pwd"}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(!text.is_empty());
}

#[tokio::test]
async fn test_bash_piped_command() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "echo -e 'a\\nb\\nc' | wc -l"}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap().trim();
    assert!(text.contains('3'));
}

#[tokio::test]
async fn test_bash_no_output() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(json!({"command": "true"}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("no output"));
}

#[tokio::test]
async fn test_bash_with_progress_channel() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut ctx = ToolUseContext::test_default();
    ctx.progress_tx = Some(tx);

    let result = BashTool
        .execute(json!({"command": "echo hello"}), &ctx)
        .await
        .unwrap();

    // Should have received at least the initial "running" progress
    let mut progress_msgs = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        progress_msgs.push(msg);
    }

    assert!(
        !progress_msgs.is_empty(),
        "should receive at least one progress message"
    );
    assert_eq!(progress_msgs[0].data["type"], "bash_progress");
    assert_eq!(progress_msgs[0].data["status"], "running");

    let text = result.data.as_str().unwrap();
    assert!(text.contains("hello"));
}

#[tokio::test]
async fn test_bash_background_without_task_handle() {
    let ctx = ToolUseContext::test_default();
    let result = BashTool
        .execute(
            json!({"command": "echo test", "run_in_background": true}),
            &ctx,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not available"));
}

// -- Stall detection tests --

#[test]
fn test_stall_prompt_yes_no() {
    assert!(coco_tool::matches_interactive_prompt(
        "Do you want to continue? (y/n)"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "output\nmore output\nContinue? [y/n]"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Are you sure? (yes/no)"
    ));
}

#[test]
fn test_stall_prompt_password() {
    assert!(coco_tool::matches_interactive_prompt("Enter password:"));
    assert!(coco_tool::matches_interactive_prompt(
        "[sudo] password for user:"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Enter passphrase for key:"
    ));
}

#[test]
fn test_stall_prompt_question_pattern() {
    assert!(coco_tool::matches_interactive_prompt(
        "Do you want to proceed?"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Would you like to overwrite?"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Are you sure you want to delete?"
    ));
}

#[test]
fn test_stall_prompt_press_key() {
    assert!(coco_tool::matches_interactive_prompt(
        "Press any key to continue"
    ));
    assert!(coco_tool::matches_interactive_prompt(
        "Press Enter to continue"
    ));
}

#[test]
fn test_stall_no_false_positive_normal_output() {
    // Normal command output should NOT match
    assert!(!coco_tool::matches_interactive_prompt(
        "Compiling project..."
    ));
    assert!(!coco_tool::matches_interactive_prompt("Build succeeded"));
    assert!(!coco_tool::matches_interactive_prompt(
        "Downloaded 42 packages"
    ));
    assert!(!coco_tool::matches_interactive_prompt("")); // empty
}

#[test]
fn test_stall_only_checks_last_line() {
    // "password:" in earlier output should not trigger
    let tail = "checking password: ok\nall tests passed\nDone.";
    assert!(!coco_tool::matches_interactive_prompt(tail));

    // But if last line has prompt, it should match
    let tail2 = "checking things\nEnter password:";
    assert!(coco_tool::matches_interactive_prompt(tail2));
}

// -- Notification format tests --

#[test]
fn test_task_notification_format() {
    let info = coco_tool::BackgroundTaskInfo {
        task_id: "task-1".into(),
        status: coco_tool::BackgroundTaskStatus::Completed,
        summary: Some("Command finished successfully".into()),
        output_file: Some("/tmp/task-1.out".into()),
        tool_use_id: Some("tu-123".into()),
        elapsed_seconds: 5.0,
        notified: false,
    };

    let xml = coco_tool::format_task_notification(&info);
    assert!(xml.contains("<task-id>task-1</task-id>"));
    assert!(xml.contains("<status>completed</status>"));
    assert!(xml.contains("<tool-use-id>tu-123</tool-use-id>"));
    assert!(xml.contains("<output-file>/tmp/task-1.out</output-file>"));
    assert!(xml.contains("<summary>Command finished successfully</summary>"));
}

#[test]
fn test_stall_notification_omits_status() {
    let stall = coco_tool::StallInfo {
        task_id: "task-2".into(),
        output_tail: "Enter password:".into(),
        frozen_seconds: 45.0,
    };

    let xml = coco_tool::format_stall_notification(&stall, Some("/tmp/task-2.out"));
    // Stall notifications must NOT have <status> tag (TS requirement)
    assert!(!xml.contains("<status>"));
    assert!(xml.contains("<task-id>task-2</task-id>"));
    assert!(xml.contains("output frozen for 45s"));
    // Raw output tail appears after XML
    assert!(xml.contains("Enter password:"));
}
