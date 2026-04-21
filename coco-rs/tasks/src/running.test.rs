use coco_types::TaskStatus;
use coco_types::TaskType;

use super::*;

#[tokio::test]
async fn test_task_create_and_get() {
    let mgr = TaskManager::new();
    let id = mgr
        .create(TaskType::LocalBash, "run tests", "/tmp/output.txt")
        .await;

    assert!(id.starts_with("tb")); // LocalBash prefix

    let task = mgr.get(&id).await.expect("task should exist");
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.description, "run tests");
}

#[tokio::test]
async fn test_task_update_status() {
    let mgr = TaskManager::new();
    let id = mgr
        .create(TaskType::LocalAgent, "agent work", "/tmp/out.txt")
        .await;

    mgr.update_status(&id, TaskStatus::Running).await;
    let task = mgr.get(&id).await.expect("task should exist");
    assert_eq!(task.status, TaskStatus::Running);

    mgr.update_status(&id, TaskStatus::Completed).await;
    let task = mgr.get(&id).await.expect("task should exist");
    assert_eq!(task.status, TaskStatus::Completed);
    assert!(task.end_time.is_some());
}

#[tokio::test]
async fn test_task_list() {
    let mgr = TaskManager::new();
    mgr.create(TaskType::LocalBash, "t1", "/tmp/1").await;
    mgr.create(TaskType::LocalBash, "t2", "/tmp/2").await;
    mgr.create(TaskType::LocalAgent, "t3", "/tmp/3").await;

    let list = mgr.list().await;
    assert_eq!(list.len(), 3);
}

#[tokio::test]
async fn test_stop_task() {
    let mgr = TaskManager::new();
    let id = mgr
        .create(TaskType::LocalBash, "long running", "/tmp/out.txt")
        .await;

    mgr.update_status(&id, TaskStatus::Running).await;
    mgr.stop(&id).await;

    let task = mgr.get(&id).await.expect("task should exist");
    assert_eq!(task.status, TaskStatus::Cancelled);
    assert!(task.end_time.is_some());
}

#[tokio::test]
async fn test_task_lifecycle() {
    let mgr = TaskManager::new();
    let id = mgr
        .create(TaskType::LocalAgent, "build project", "/tmp/build.txt")
        .await;

    // Pending -> Running -> Completed
    let task = mgr.get(&id).await.expect("task should exist");
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.end_time.is_none());

    mgr.update_status(&id, TaskStatus::Running).await;
    let task = mgr.get(&id).await.expect("task should exist");
    assert_eq!(task.status, TaskStatus::Running);
    assert!(task.end_time.is_none());

    mgr.update_status(&id, TaskStatus::Completed).await;
    let task = mgr.get(&id).await.expect("task should exist");
    assert_eq!(task.status, TaskStatus::Completed);
    assert!(task.end_time.is_some());
    assert!(task.status.is_terminal());
}

#[tokio::test]
async fn test_output_storage() {
    let mgr = TaskManager::new();
    let id = mgr
        .create(TaskType::LocalBash, "echo hello", "/tmp/echo.txt")
        .await;

    // No output initially
    assert!(mgr.get_output(&id).await.is_none());

    let output = TaskOutput {
        stdout: "hello world\n".to_string(),
        stderr: String::new(),
        exit_code: 0,
    };
    mgr.set_output(&id, output).await;

    let retrieved = mgr.get_output(&id).await.expect("output should exist");
    assert_eq!(retrieved.stdout, "hello world\n");
    assert!(retrieved.stderr.is_empty());
    assert_eq!(retrieved.exit_code, 0);
}

#[tokio::test]
async fn test_remove_completed() {
    let mgr = TaskManager::new();
    let id1 = mgr.create(TaskType::LocalBash, "done", "/tmp/1.txt").await;
    let id2 = mgr
        .create(TaskType::LocalBash, "still running", "/tmp/2.txt")
        .await;
    let id3 = mgr
        .create(TaskType::LocalBash, "failed", "/tmp/3.txt")
        .await;

    mgr.update_status(&id1, TaskStatus::Completed).await;
    mgr.update_status(&id2, TaskStatus::Running).await;
    mgr.update_status(&id3, TaskStatus::Failed).await;

    mgr.set_output(
        &id1,
        TaskOutput {
            stdout: "ok".into(),
            stderr: String::new(),
            exit_code: 0,
        },
    )
    .await;

    let removed = mgr.remove_completed().await;
    assert_eq!(removed, 2); // id1 (Completed) + id3 (Failed)

    // Only the running task remains
    let remaining = mgr.list().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, id2);

    // Output for completed task should also be removed
    assert!(mgr.get_output(&id1).await.is_none());
}

// ─── WS-6: event sink emission ─────────────────────────────────────────
//
// When constructed with `with_event_sink(tx)`, TaskManager emits the
// matching `ServerNotification::TaskStarted/TaskProgress/TaskCompleted`
// for every lifecycle transition. Tests exercise the full round-trip
// for one happy-path task plus one failure path.

use coco_types::CoreEvent;
use coco_types::ServerNotification;
use coco_types::TaskCompletionStatus;
use tokio::sync::mpsc;

fn collect(rx: &mut mpsc::Receiver<CoreEvent>) -> Vec<ServerNotification> {
    let mut out = Vec::new();
    while let Ok(evt) = rx.try_recv() {
        if let CoreEvent::Protocol(n) = evt {
            out.push(n);
        }
    }
    out
}

#[tokio::test]
async fn test_event_sink_happy_path() {
    let (tx, mut rx) = mpsc::channel::<CoreEvent>(16);
    let mgr = TaskManager::new().with_event_sink(tx);

    let id = mgr
        .create(TaskType::LocalAgent, "build project", "/tmp/build.txt")
        .await;
    mgr.update_status(&id, TaskStatus::Running).await;
    mgr.update_status(&id, TaskStatus::Completed).await;

    let events = collect(&mut rx);
    assert_eq!(events.len(), 3, "expected 3 events, got: {events:?}");

    match &events[0] {
        ServerNotification::TaskStarted(p) => {
            assert_eq!(p.task_id, id);
            assert_eq!(p.description, "build project");
            assert_eq!(p.task_type.as_deref(), Some("local_agent"));
        }
        other => panic!("expected TaskStarted, got {other:?}"),
    }
    match &events[1] {
        ServerNotification::TaskProgress(p) => {
            assert_eq!(p.task_id, id);
            assert_eq!(p.description, "build project");
        }
        other => panic!("expected TaskProgress, got {other:?}"),
    }
    match &events[2] {
        ServerNotification::TaskCompleted(p) => {
            assert_eq!(p.task_id, id);
            assert_eq!(p.status, TaskCompletionStatus::Completed);
            assert_eq!(p.output_file, "/tmp/build.txt");
        }
        other => panic!("expected TaskCompleted, got {other:?}"),
    }
}

#[tokio::test]
async fn test_event_sink_failure_maps_to_failed_status() {
    let (tx, mut rx) = mpsc::channel::<CoreEvent>(16);
    let mgr = TaskManager::new().with_event_sink(tx);

    let id = mgr
        .create(TaskType::LocalBash, "flaky script", "/tmp/out.txt")
        .await;
    mgr.update_status(&id, TaskStatus::Failed).await;

    let events = collect(&mut rx);
    assert_eq!(events.len(), 2);
    match &events[1] {
        ServerNotification::TaskCompleted(p) => {
            assert_eq!(p.status, TaskCompletionStatus::Failed);
        }
        other => panic!("expected TaskCompleted(Failed), got {other:?}"),
    }
}

#[tokio::test]
async fn test_event_sink_stop_maps_to_stopped_status() {
    let (tx, mut rx) = mpsc::channel::<CoreEvent>(16);
    let mgr = TaskManager::new().with_event_sink(tx);

    let id = mgr
        .create(TaskType::LocalBash, "long job", "/tmp/out.txt")
        .await;
    mgr.stop(&id).await;

    let events = collect(&mut rx);
    assert_eq!(events.len(), 2);
    match &events[1] {
        ServerNotification::TaskCompleted(p) => {
            assert_eq!(p.status, TaskCompletionStatus::Stopped);
        }
        other => panic!("expected TaskCompleted(Stopped), got {other:?}"),
    }
}

#[tokio::test]
async fn test_no_emission_without_sink() {
    // Default (no sink) must not panic and must not emit anything.
    let mgr = TaskManager::new();
    let id = mgr
        .create(TaskType::LocalBash, "no sink", "/tmp/out.txt")
        .await;
    mgr.update_status(&id, TaskStatus::Running).await;
    mgr.update_status(&id, TaskStatus::Completed).await;
    // Nothing observable to assert — just that these calls don't panic
    // and the task state updates normally.
    let task = mgr.get(&id).await.expect("task exists");
    assert_eq!(task.status, TaskStatus::Completed);
}
