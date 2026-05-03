use super::*;
use std::sync::Arc;

fn rt() -> Arc<TaskRuntime> {
    Arc::new(TaskRuntime::new(Arc::new(coco_tasks::TaskManager::new())))
}

#[tokio::test]
async fn register_creates_running_task_with_tool_use_id() {
    let rt = rt();
    let cancel = CancellationToken::new();
    let task_id = rt
        .register_agent_task("explore something", Some("toolu_01"), cancel)
        .await;

    let info = rt.get_task_status(&task_id).await.unwrap();
    assert_eq!(info.task_id, task_id);
    assert_eq!(info.status, BackgroundTaskStatus::Running);
    assert_eq!(info.tool_use_id.as_deref(), Some("toolu_01"));
    assert_eq!(info.summary.as_deref(), Some("explore something"));
}

#[tokio::test]
async fn output_delta_returns_appended_chunks_with_offset() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;

    rt.append_output(&task_id, "first ").await;
    rt.append_output(&task_id, "second").await;

    // Initial read from offset 0 returns the full buffer.
    let delta1 = rt.get_task_output_delta(&task_id, 0).await.unwrap();
    assert_eq!(delta1.content, "first second");
    assert_eq!(delta1.new_offset, 12);
    assert!(!delta1.is_complete);

    // Subsequent read from offset 12 returns nothing new.
    let delta2 = rt
        .get_task_output_delta(&task_id, delta1.new_offset)
        .await
        .unwrap();
    assert!(delta2.content.is_empty());
    assert!(!delta2.is_complete);
}

#[tokio::test]
async fn mark_completed_sets_terminal_status_and_appends_response() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;

    rt.mark_completed(&task_id, Some("final answer")).await;

    let info = rt.get_task_status(&task_id).await.unwrap();
    assert_eq!(info.status, BackgroundTaskStatus::Completed);
    let delta = rt.get_task_output_delta(&task_id, 0).await.unwrap();
    assert_eq!(delta.content, "final answer");
    assert!(delta.is_complete);
}

#[tokio::test]
async fn mark_failed_appends_error_and_flips_status() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;

    rt.mark_failed(&task_id, "transport crash").await;

    let info = rt.get_task_status(&task_id).await.unwrap();
    assert_eq!(info.status, BackgroundTaskStatus::Failed);
    let delta = rt.get_task_output_delta(&task_id, 0).await.unwrap();
    assert!(delta.content.contains("transport crash"));
    assert!(delta.is_complete);
}

#[tokio::test]
async fn kill_task_cancels_token_and_marks_killed() {
    let rt = rt();
    let cancel = CancellationToken::new();
    let task_id = rt.register_agent_task("work", None, cancel.clone()).await;

    rt.kill_task(&task_id).await.unwrap();

    assert!(cancel.is_cancelled(), "cancel token must fire");
    let info = rt.get_task_status(&task_id).await.unwrap();
    assert_eq!(info.status, BackgroundTaskStatus::Killed);
}

#[tokio::test]
async fn list_tasks_returns_all_registered() {
    let rt = rt();
    let _ = rt
        .register_agent_task("a", None, CancellationToken::new())
        .await;
    let _ = rt
        .register_agent_task("b", None, CancellationToken::new())
        .await;
    assert_eq!(rt.list_tasks().await.len(), 2);
}

#[tokio::test]
async fn poll_notifications_emits_terminal_tasks_once() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;
    rt.mark_completed(&task_id, None).await;

    let first = rt.poll_notifications().await;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].task_id, task_id);

    // Second poll: notified flag prevents re-emit.
    let second = rt.poll_notifications().await;
    assert!(second.is_empty());
}

#[tokio::test]
async fn unknown_task_id_errors() {
    let rt = rt();
    assert!(rt.get_task_status("ghost").await.is_err());
    assert!(rt.get_task_output_delta("ghost", 0).await.is_err());
    assert!(rt.kill_task("ghost").await.is_err());
}

#[tokio::test]
async fn mark_completed_cancels_per_task_token() {
    // Closes the timer-leak window: when an engine completes
    // naturally, the periodic-summary timer must exit promptly
    // rather than waiting up to 30 s for the next is_terminal poll.
    let rt = rt();
    let cancel = CancellationToken::new();
    let task_id = rt.register_agent_task("work", None, cancel.clone()).await;

    assert!(!cancel.is_cancelled(), "token starts un-cancelled");
    rt.mark_completed(&task_id, None).await;
    assert!(
        cancel.is_cancelled(),
        "mark_completed must fire the per-task cancel token so timers exit"
    );
}

#[tokio::test]
async fn mark_failed_cancels_per_task_token() {
    let rt = rt();
    let cancel = CancellationToken::new();
    let task_id = rt.register_agent_task("work", None, cancel.clone()).await;

    rt.mark_failed(&task_id, "boom").await;
    assert!(
        cancel.is_cancelled(),
        "mark_failed must fire the per-task cancel token"
    );
}

#[tokio::test]
async fn append_output_persists_to_disk_and_reads_via_pread() {
    // TS-aligned: append accumulates on disk; read_delta retrieves
    // the same bytes via offset-based pread.
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;

    rt.append_output(&task_id, "hello ").await;
    rt.append_output(&task_id, "world").await;

    // Flush the drain so the on-disk file is up to date before read.
    let delta = rt.get_task_output_delta(&task_id, 0).await.unwrap();
    assert_eq!(delta.content, "hello world");
    assert_eq!(delta.new_offset, 11);
    let delta2 = rt.get_task_output_delta(&task_id, 11).await.unwrap();
    assert!(delta2.content.is_empty());
}

#[tokio::test]
async fn read_output_returns_full_buffer_after_completion() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;
    rt.append_output(&task_id, "alpha").await;
    rt.mark_completed(&task_id, Some(" beta")).await;
    let buf = rt.read_output(&task_id).await;
    assert!(buf.starts_with("alpha"));
    assert!(buf.contains("beta"));
}

#[tokio::test]
async fn shell_spawn_returns_explicit_unsupported() {
    let rt = rt();
    let result = rt
        .spawn_shell_task(BackgroundShellRequest {
            command: "ls".into(),
            timeout_ms: None,
            description: None,
        })
        .await;
    assert!(result.is_err());
}
