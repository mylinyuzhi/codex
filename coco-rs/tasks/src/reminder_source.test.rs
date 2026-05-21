use super::*;
use crate::running::TaskManager;
use coco_system_reminder::TaskStatusSource;
use coco_types::TaskStatus;
use coco_types::TaskType;

#[tokio::test]
async fn collect_returns_empty_when_not_post_compact() {
    let mgr = TaskManager::new();
    mgr.create(TaskType::LocalAgent, "foo", "/tmp/foo.log")
        .await;
    let out = mgr.collect(None, /*just_compacted=*/ false).await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn collect_emits_snapshot_post_compact_for_running() {
    let mgr = TaskManager::new();
    let _id = mgr
        .create(TaskType::LocalAgent, "scan repo", "/tmp/scan.log")
        .await;
    let out = mgr.collect(None, true).await;
    assert_eq!(out.len(), 1);
    let s = &out[0];
    assert_eq!(s.description, "scan repo");
    assert_eq!(s.output_file_path.as_deref(), Some("/tmp/scan.log"));
    assert!(
        matches!(s.status, coco_system_reminder::TaskRunStatus::Running),
        "post-compact snapshot must collapse to Running per W6/A2"
    );
}

/// W6 / A2: terminal tasks (Completed / Failed / Killed) must NOT
/// appear in the post-compact reminder. They've already delivered via
/// the `QueuedCommandGenerator` (`<task-notification>` envelope), so
/// re-emitting them would double-inform the model.
#[tokio::test]
async fn collect_skips_terminal_tasks_post_compact() {
    let mgr = TaskManager::new();
    let id_completed = mgr
        .create_running(TaskType::LocalAgent, "done", "/tmp/done.log")
        .await;
    let id_failed = mgr
        .create_running(TaskType::LocalAgent, "broke", "/tmp/broke.log")
        .await;
    let id_killed = mgr
        .create_running(TaskType::LocalAgent, "stopped", "/tmp/stopped.log")
        .await;
    let id_running = mgr
        .create_running(TaskType::LocalAgent, "alive", "/tmp/alive.log")
        .await;

    mgr.update_status(&id_completed, TaskStatus::Completed)
        .await;
    mgr.update_status(&id_failed, TaskStatus::Failed).await;
    mgr.update_status(&id_killed, TaskStatus::Killed).await;

    let out = mgr.collect(None, true).await;
    assert_eq!(
        out.len(),
        1,
        "only the still-Running task should appear in post-compact reminder"
    );
    assert_eq!(out[0].task_id, id_running);
    let _ = id_completed;
    let _ = id_failed;
    let _ = id_killed;
}

#[tokio::test]
async fn status_mapping_collapses_5_to_4() {
    // TS has 5 statuses (Task.ts:15-21); the reminder generator
    // ignores Pending/Running distinction (both render as Running).
    assert!(matches!(
        map_status(coco_types::TaskStatus::Completed),
        coco_system_reminder::TaskRunStatus::Completed
    ));
    assert!(matches!(
        map_status(coco_types::TaskStatus::Failed),
        coco_system_reminder::TaskRunStatus::Failed
    ));
    assert!(matches!(
        map_status(coco_types::TaskStatus::Killed),
        coco_system_reminder::TaskRunStatus::Killed
    ));
    assert!(matches!(
        map_status(coco_types::TaskStatus::Running),
        coco_system_reminder::TaskRunStatus::Running
    ));
    assert!(matches!(
        map_status(coco_types::TaskStatus::Pending),
        coco_system_reminder::TaskRunStatus::Running
    ));
}
