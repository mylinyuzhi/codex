use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn setup() -> (TempDir, TempDir) {
    let config_home = TempDir::new().expect("create config dir");
    let work_dir = TempDir::new().expect("create work dir");
    (config_home, work_dir)
}

#[tokio::test]
async fn test_track_edit_creates_backup() {
    let (config_home, work_dir) = setup();
    let file = work_dir.path().join("hello.txt");
    tokio::fs::write(&file, "original").await.unwrap();

    let mut state = FileHistoryState::new();
    state
        .track_edit(&file, "msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    assert!(state.tracked_files.contains(&file));
    assert_eq!(state.snapshots.len(), 1);

    let backup = state.latest_backup(&file).unwrap();
    assert!(backup.backup_file_name.is_some());
    assert_eq!(backup.version, 1);
}

#[tokio::test]
async fn test_track_edit_nonexistent_file_records_null() {
    let (config_home, _work_dir) = setup();
    let file = PathBuf::from("/tmp/coco-test-nonexistent-12345.txt");

    let mut state = FileHistoryState::new();
    state
        .track_edit(&file, "msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    let backup = state.latest_backup(&file).unwrap();
    assert!(backup.backup_file_name.is_none());
}

#[tokio::test]
async fn test_track_edit_idempotent_same_message() {
    let (config_home, work_dir) = setup();
    let file = work_dir.path().join("hello.txt");
    tokio::fs::write(&file, "original").await.unwrap();

    let mut state = FileHistoryState::new();
    state
        .track_edit(&file, "msg-1", config_home.path(), "session-1")
        .await
        .unwrap();
    state
        .track_edit(&file, "msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    // Should still have only one snapshot entry for this file.
    assert_eq!(state.snapshots.len(), 1);
    assert_eq!(state.snapshots[0].tracked_file_backups.len(), 1);
}

#[tokio::test]
async fn test_make_snapshot_backs_up_tracked_files() {
    let (config_home, work_dir) = setup();
    let file = work_dir.path().join("data.txt");
    tokio::fs::write(&file, "v1 content").await.unwrap();

    let mut state = FileHistoryState::new();
    state.track_file(file.clone());
    state
        .make_snapshot("msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    assert_eq!(state.snapshots.len(), 1);
    let backup = state.latest_backup(&file).unwrap();
    assert!(backup.backup_file_name.is_some());
}

#[tokio::test]
async fn test_rewind_restores_file_content() {
    let (config_home, work_dir) = setup();
    let file = work_dir.path().join("code.rs");
    tokio::fs::write(&file, "fn original()").await.unwrap();

    let mut state = FileHistoryState::new();
    state
        .track_edit(&file, "msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    // Modify the file (simulating a tool write).
    tokio::fs::write(&file, "fn modified()").await.unwrap();

    // Rewind to msg-1.
    let changed = state
        .rewind("msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    assert_eq!(changed.len(), 1);
    let content = tokio::fs::read_to_string(&file).await.unwrap();
    assert_eq!(content, "fn original()");
}

#[tokio::test]
async fn test_rewind_deletes_file_that_didnt_exist() {
    let (config_home, work_dir) = setup();
    let file = work_dir.path().join("new_file.txt");

    let mut state = FileHistoryState::new();
    // Track before file exists (null backup).
    state
        .track_edit(&file, "msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    // Create the file (simulating tool creating it).
    tokio::fs::write(&file, "created content").await.unwrap();

    // Rewind — file should be deleted.
    let changed = state
        .rewind("msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    assert_eq!(changed.len(), 1);
    assert!(!file.exists());
}

#[tokio::test]
async fn test_has_any_changes_detects_modification() {
    let (config_home, work_dir) = setup();
    let file = work_dir.path().join("check.txt");
    tokio::fs::write(&file, "before").await.unwrap();

    let mut state = FileHistoryState::new();
    state
        .track_edit(&file, "msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    // No changes yet.
    assert!(
        !state
            .has_any_changes("msg-1", config_home.path(), "session-1")
            .await
    );

    // Modify file.
    tokio::fs::write(&file, "after").await.unwrap();
    assert!(
        state
            .has_any_changes("msg-1", config_home.path(), "session-1")
            .await
    );
}

#[tokio::test]
async fn test_get_diff_stats() {
    let (config_home, work_dir) = setup();
    let file = work_dir.path().join("stats.txt");
    tokio::fs::write(&file, "line1\nline2\n").await.unwrap();

    let mut state = FileHistoryState::new();
    state
        .track_edit(&file, "msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    tokio::fs::write(&file, "line1\nline2\nline3\nline4\n")
        .await
        .unwrap();

    let stats = state
        .get_diff_stats("msg-1", config_home.path(), "session-1")
        .await
        .unwrap();

    assert_eq!(stats.files_changed.len(), 1);
    assert_eq!(stats.insertions, 2);
}

#[tokio::test]
async fn test_can_restore() {
    let state = FileHistoryState {
        snapshots: vec![FileHistorySnapshot {
            message_id: "msg-42".to_string(),
            tracked_file_backups: HashMap::new(),
            timestamp: 0,
        }],
        ..Default::default()
    };
    assert!(state.can_restore("msg-42"));
    assert!(!state.can_restore("msg-99"));
}

#[tokio::test]
async fn test_snapshot_cap_enforcement() {
    let (config_home, work_dir) = setup();
    let file = work_dir.path().join("cap.txt");
    tokio::fs::write(&file, "data").await.unwrap();

    let mut state = FileHistoryState::new();
    state.track_file(file.clone());

    for i in 0..=MAX_SNAPSHOTS + 5 {
        state
            .make_snapshot(&format!("msg-{i}"), config_home.path(), "session-1")
            .await
            .unwrap();
    }

    assert!(state.snapshots.len() <= MAX_SNAPSHOTS);
}

#[tokio::test]
async fn test_copy_file_history_for_resume() {
    let config_home = TempDir::new().unwrap();
    let src_dir = backup_dir(config_home.path(), "old-session");
    tokio::fs::create_dir_all(&src_dir).await.unwrap();
    tokio::fs::write(src_dir.join("backup1"), "data1")
        .await
        .unwrap();
    tokio::fs::write(src_dir.join("backup2"), "data2")
        .await
        .unwrap();

    let copied = copy_file_history_for_resume(config_home.path(), "old-session", "new-session")
        .await
        .unwrap();

    assert_eq!(copied, 2);
    let dst_dir = backup_dir(config_home.path(), "new-session");
    assert!(dst_dir.join("backup1").exists());
    assert!(dst_dir.join("backup2").exists());
}

#[tokio::test]
async fn test_restore_from_snapshots() {
    let snapshots = vec![
        FileHistorySnapshot {
            message_id: "m1".to_string(),
            tracked_file_backups: {
                let mut m = HashMap::new();
                m.insert(
                    PathBuf::from("/a.txt"),
                    FileHistoryBackup {
                        backup_file_name: Some("abc@v1".to_string()),
                        version: 1,
                        backup_time: 100,
                    },
                );
                m
            },
            timestamp: 100,
        },
        FileHistorySnapshot {
            message_id: "m2".to_string(),
            tracked_file_backups: {
                let mut m = HashMap::new();
                m.insert(
                    PathBuf::from("/a.txt"),
                    FileHistoryBackup {
                        backup_file_name: Some("abc@v2".to_string()),
                        version: 2,
                        backup_time: 200,
                    },
                );
                m.insert(
                    PathBuf::from("/b.txt"),
                    FileHistoryBackup {
                        backup_file_name: Some("def@v1".to_string()),
                        version: 1,
                        backup_time: 200,
                    },
                );
                m
            },
            timestamp: 200,
        },
    ];

    let state = FileHistoryState::restore_from_snapshots(snapshots);
    assert_eq!(state.snapshots.len(), 2);
    assert_eq!(state.tracked_files.len(), 2);
    assert_eq!(state.snapshot_sequence, 2);
}
