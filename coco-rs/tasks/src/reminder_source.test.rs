use super::*;
use crate::running::TaskManager;
use coco_system_reminder::TaskStatusSource;
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
async fn collect_emits_snapshot_post_compact() {
    let mgr = TaskManager::new();
    let _id = mgr
        .create(TaskType::LocalAgent, "scan repo", "/tmp/scan.log")
        .await;
    let out = mgr.collect(None, true).await;
    assert_eq!(out.len(), 1);
    let s = &out[0];
    assert_eq!(s.description, "scan repo");
    assert_eq!(s.output_file_path.as_deref(), Some("/tmp/scan.log"));
}

#[tokio::test]
async fn status_mapping_collapses_6_to_4() {
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
        map_status(coco_types::TaskStatus::Cancelled),
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
