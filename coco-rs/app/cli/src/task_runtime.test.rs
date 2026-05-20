use super::*;
use coco_tool_runtime::{
    AgentCompletionPayload, AgentTaskRegistry, AgentUsage, AgentWorktree, BackgroundShellRequest,
    ShellTaskSpawner, TaskController, TaskReader,
};
use std::sync::Arc;

fn rt() -> Arc<TaskRuntime> {
    Arc::new(TaskRuntime::new(Arc::new(coco_tasks::TaskManager::new())))
}

/// Capture-everything sink for assertions in tests.
#[derive(Default, Clone)]
struct CapturingSink {
    captured: Arc<tokio::sync::Mutex<Vec<coco_tasks::TaskNotification>>>,
}

#[async_trait::async_trait]
impl coco_tasks::NotificationSink for CapturingSink {
    async fn push(&self, n: coco_tasks::TaskNotification) {
        self.captured.lock().await.push(n);
    }
}

fn rt_with_sink(sink: CapturingSink) -> Arc<TaskRuntime> {
    Arc::new(
        TaskRuntime::new(Arc::new(coco_tasks::TaskManager::new()))
            .with_notification_sink(Arc::new(sink) as coco_tasks::NotificationSinkRef),
    )
}

#[tokio::test]
async fn register_creates_running_task_with_tool_use_id() {
    let rt = rt();
    let cancel = CancellationToken::new();
    let task_id = rt
        .register_agent_task("explore something", Some("toolu_01"), cancel)
        .await;

    let state = rt.get_task_status(&task_id).await.unwrap();
    assert_eq!(state.id, task_id);
    assert_eq!(state.status, TaskStatus::Running);
    assert_eq!(state.tool_use_id.as_deref(), Some("toolu_01"));
    assert_eq!(state.description, "explore something");
}

#[tokio::test]
async fn output_delta_returns_appended_chunks_with_offset() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;

    rt.append_output(&task_id, "first ").await;
    rt.append_output(&task_id, "second").await;

    let delta1 = rt.get_task_output_delta(&task_id, 0).await.unwrap();
    assert_eq!(delta1.content, "first second");
    assert_eq!(delta1.new_offset, 12);
    assert!(!delta1.is_complete);

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
    rt.mark_completed(
        &task_id,
        AgentCompletionPayload {
            result: Some("final answer".into()),
            ..Default::default()
        },
    )
    .await;
    let state = rt.get_task_status(&task_id).await.unwrap();
    assert_eq!(state.status, TaskStatus::Completed);
    let delta = rt.get_task_output_delta(&task_id, 0).await.unwrap();
    assert_eq!(delta.content, "final answer");
    assert!(delta.is_complete);
}

#[tokio::test]
async fn mark_completed_pushes_rich_agent_notification() {
    let sink = CapturingSink::default();
    let captured = sink.captured.clone();
    let rt = rt_with_sink(sink);
    let task_id = rt
        .register_agent_task("build", Some("toolu_x"), CancellationToken::new())
        .await;
    rt.mark_completed(
        &task_id,
        AgentCompletionPayload {
            result: Some("done".into()),
            usage: Some(AgentUsage {
                total_tokens: 500,
                tool_uses: 3,
                duration_ms: 6000,
            }),
            worktree: Some(AgentWorktree {
                path: "/tmp/wt".into(),
                branch: Some("feat/x".into()),
            }),
        },
    )
    .await;
    let captured = captured.lock().await;
    assert_eq!(captured.len(), 1);
    let kind = &captured[0].kind;
    let coco_tasks::NotificationKind::AgentTerminal {
        status,
        result,
        usage,
        worktree,
        ..
    } = kind
    else {
        panic!("expected AgentTerminal, got {kind:?}");
    };
    assert_eq!(*status, coco_tasks::TerminalStatus::Completed);
    assert_eq!(result.as_deref(), Some("done"));
    assert_eq!(usage.as_ref().unwrap().total_tokens, 500);
    assert_eq!(worktree.as_ref().unwrap().branch.as_deref(), Some("feat/x"));
    assert_eq!(captured[0].tool_use_id.as_deref(), Some("toolu_x"));
}

#[tokio::test]
async fn mark_failed_appends_error_and_flips_status() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;
    rt.mark_failed(&task_id, "transport crash").await;
    let state = rt.get_task_status(&task_id).await.unwrap();
    assert_eq!(state.status, TaskStatus::Failed);
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
    let state = rt.get_task_status(&task_id).await.unwrap();
    assert_eq!(state.status, TaskStatus::Killed);
}

#[tokio::test]
async fn kill_task_pushes_killed_notification() {
    let sink = CapturingSink::default();
    let captured = sink.captured.clone();
    let rt = rt_with_sink(sink);
    let task_id = rt
        .register_agent_task("explore", None, CancellationToken::new())
        .await;
    rt.kill_task(&task_id).await.unwrap();
    let captured = captured.lock().await;
    assert_eq!(captured.len(), 1);
    matches!(
        captured[0].kind,
        coco_tasks::NotificationKind::AgentTerminal {
            status: coco_tasks::TerminalStatus::Killed,
            ..
        }
    );
}

#[tokio::test]
async fn subscribe_terminal_fires_on_mark_completed() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;
    let signal = rt
        .subscribe_terminal(&task_id)
        .await
        .expect("entry must exist");
    let rt2 = rt.clone();
    let id2 = task_id.clone();
    let _handle = tokio::spawn(async move {
        rt2.mark_completed(&id2, AgentCompletionPayload::default())
            .await;
    });
    let final_status =
        tokio::time::timeout(std::time::Duration::from_secs(2), signal.await_terminal())
            .await
            .expect("terminal signal must fire within 2s");
    assert!(final_status.is_terminal());
}

#[tokio::test]
async fn subscribe_terminal_already_terminal_returns_immediately() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;
    rt.mark_completed(&task_id, AgentCompletionPayload::default())
        .await;
    // After termination, subscriber should see the stored terminal value.
    let signal = rt
        .subscribe_terminal(&task_id)
        .await
        .expect("entry retained");
    let s = tokio::time::timeout(std::time::Duration::from_secs(1), signal.await_terminal())
        .await
        .expect("should resolve immediately");
    assert!(s.is_terminal());
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
async fn unknown_task_id_errors() {
    let rt = rt();
    assert!(rt.get_task_status("ghost").await.is_err());
    assert!(rt.get_task_output_delta("ghost", 0).await.is_err());
    assert!(rt.kill_task("ghost").await.is_err());
    assert!(rt.subscribe_terminal("ghost").await.is_none());
}

#[tokio::test]
async fn mark_completed_cancels_per_task_token() {
    let rt = rt();
    let cancel = CancellationToken::new();
    let task_id = rt.register_agent_task("work", None, cancel.clone()).await;
    assert!(!cancel.is_cancelled());
    rt.mark_completed(&task_id, AgentCompletionPayload::default())
        .await;
    assert!(cancel.is_cancelled());
}

#[tokio::test]
async fn mark_failed_cancels_per_task_token() {
    let rt = rt();
    let cancel = CancellationToken::new();
    let task_id = rt.register_agent_task("work", None, cancel.clone()).await;
    rt.mark_failed(&task_id, "boom").await;
    assert!(cancel.is_cancelled());
}

#[cfg(not(windows))]
#[tokio::test]
async fn shell_spawn_runs_command_and_marks_completed() {
    let rt = rt();
    let task_id = rt
        .spawn_shell_task(BackgroundShellRequest {
            command: "echo hello-bg".into(),
            timeout_ms: Some(5_000),
            description: "echo test".into(),
            tool_use_id: Some("toolu_sh1".into()),
            agent_id: None,
        })
        .await
        .expect("shell spawn should succeed");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let state = rt.get_task_status(&task_id).await.unwrap();
        if state.status.is_terminal() {
            assert_eq!(state.status, TaskStatus::Completed);
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("shell task did not complete within 5s");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let delta = rt.get_task_output_delta(&task_id, 0).await.unwrap();
    assert!(delta.content.contains("hello-bg"));
    assert!(delta.is_complete);
}

#[cfg(not(windows))]
#[tokio::test]
async fn shell_spawn_propagates_nonzero_exit_as_failed() {
    let rt = rt();
    let task_id = rt
        .spawn_shell_task(BackgroundShellRequest {
            command: "exit 7".into(),
            timeout_ms: Some(5_000),
            description: "fail".into(),
            tool_use_id: None,
            agent_id: None,
        })
        .await
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let final_status = loop {
        let state = rt.get_task_status(&task_id).await.unwrap();
        if state.status.is_terminal() {
            break state.status;
        }
        if std::time::Instant::now() > deadline {
            panic!("task did not exit");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert_eq!(final_status, TaskStatus::Failed);
}

#[cfg(not(windows))]
#[tokio::test]
async fn shell_spawn_threads_tool_use_id_and_agent_id_into_notification() {
    let sink = CapturingSink::default();
    let captured = sink.captured.clone();
    let rt = rt_with_sink(sink);

    let _task_id = rt
        .spawn_shell_task(BackgroundShellRequest {
            command: "true".into(),
            timeout_ms: Some(5_000),
            description: "noop".into(),
            tool_use_id: Some("toolu_bash99".into()),
            agent_id: Some("agent-3".into()),
        })
        .await
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if !captured.lock().await.is_empty() {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("notification didn't arrive");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let captured = captured.lock().await;
    let n = &captured[0];
    assert_eq!(n.tool_use_id.as_deref(), Some("toolu_bash99"));
    assert_eq!(n.agent_id.as_deref(), Some("agent-3"));
    matches!(
        n.kind,
        coco_tasks::NotificationKind::ShellTerminal {
            status: coco_tasks::TerminalStatus::Completed,
            ..
        }
    );
}

#[tokio::test]
async fn no_sink_means_no_panic_on_terminal() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("explore", None, CancellationToken::new())
        .await;
    rt.mark_completed(&task_id, AgentCompletionPayload::default())
        .await;
}

#[tokio::test]
async fn read_output_returns_full_buffer_after_completion() {
    let rt = rt();
    let task_id = rt
        .register_agent_task("work", None, CancellationToken::new())
        .await;
    rt.append_output(&task_id, "alpha").await;
    rt.mark_completed(
        &task_id,
        AgentCompletionPayload {
            result: Some(" beta".into()),
            ..Default::default()
        },
    )
    .await;
    let buf = rt.read_output(&task_id).await;
    assert!(buf.starts_with("alpha"));
    assert!(buf.contains("beta"));
}
