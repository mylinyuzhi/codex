use super::*;
use tempfile::TempDir;

async fn create_test_manager(tmp: &TempDir, is_git: bool) -> SnapshotManager {
    create_test_manager_with_limit(tmp, is_git, DEFAULT_MAX_SNAPSHOTS).await
}

async fn create_test_manager_with_limit(
    tmp: &TempDir,
    is_git: bool,
    max_snapshots: usize,
) -> SnapshotManager {
    let backup_dir = tmp.path().join("backups");
    let store = Arc::new(FileBackupStore::with_dir(backup_dir).await.unwrap());
    SnapshotManager::with_max_snapshots(
        store,
        tmp.path().to_path_buf(),
        is_git,
        GhostConfig::default(),
        max_snapshots,
    )
}

#[tokio::test]
async fn test_non_git_rewind() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"original").await.unwrap();

    // Start turn and backup
    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();

    // Modify file
    tokio::fs::write(&file, b"modified").await.unwrap();

    // Finalize
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;
    assert!(mgr.has_snapshots().await);

    // Rewind
    let result = mgr.rewind_last_turn().await.unwrap();
    assert_eq!(result.rewound_turn, 1);
    assert!(!result.used_git_restore);
    assert_eq!(result.restored_files.len(), 1);

    // File should be restored
    let content = tokio::fs::read_to_string(&file).await.unwrap();
    assert_eq!(content, "original");

    // Rewind info should be available
    let info = mgr.take_rewind_info().await.unwrap();
    assert_eq!(info.rewound_turn_number, 1);
    assert_eq!(info.restored_file_count, 1);

    // Second take should return None
    assert!(mgr.take_rewind_info().await.is_none());
}

#[tokio::test]
async fn test_rewind_no_snapshots() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let result = mgr.rewind_last_turn().await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("No snapshots available")
    );
}

#[tokio::test]
async fn test_compaction_boundary() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"v1").await.unwrap();

    // Create snapshots for turns 1 and 2
    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    tokio::fs::write(&file, b"v2").await.unwrap();
    mgr.start_turn_snapshot("turn-2", 2, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    mgr.finalize_turn_snapshot("turn-2", 2, None).await;

    // Set compaction boundary at turn 1
    mgr.set_compaction_boundary(1).await;

    // Turn 2 should still be rewindable
    assert!(mgr.has_snapshots().await);

    // But after rewinding turn 2, turn 1 should be gone
    let result = mgr.rewind_last_turn().await.unwrap();
    assert_eq!(result.rewound_turn, 2);
    assert!(!mgr.has_snapshots().await);
}

#[tokio::test]
async fn test_snapshot_serialization() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"data").await.unwrap();

    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Serialize
    let json = mgr.serialize_snapshots().await.unwrap();

    // Create new manager and restore
    let mgr2 = create_test_manager(&tmp, false).await;
    mgr2.restore_snapshots(&json).await.unwrap();
    assert!(mgr2.has_snapshots().await);
    assert_eq!(mgr2.last_snapshot_turn().await, Some(1));
}

#[tokio::test]
async fn test_multiple_turns_rewind_order() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"v0").await.unwrap();

    // Turn 1
    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, b"v1").await.unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Turn 2
    mgr.start_turn_snapshot("turn-2", 2, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, b"v2").await.unwrap();
    mgr.finalize_turn_snapshot("turn-2", 2, None).await;

    // Rewind turn 2 first (LIFO)
    let r2 = mgr.rewind_last_turn().await.unwrap();
    assert_eq!(r2.rewound_turn, 2);
    assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), "v1");

    // Rewind turn 1
    let r1 = mgr.rewind_last_turn().await.unwrap();
    assert_eq!(r1.rewound_turn, 1);
    assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), "v0");

    // No more snapshots
    assert!(!mgr.has_snapshots().await);
}

#[tokio::test]
async fn test_max_snapshots_retention() {
    let tmp = TempDir::new().unwrap();
    // Limit to 2 snapshots
    let mgr = create_test_manager_with_limit(&tmp, false, 2).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"v0").await.unwrap();

    // Turn 1
    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, b"v1").await.unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Turn 2
    mgr.start_turn_snapshot("turn-2", 2, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, b"v2").await.unwrap();
    mgr.finalize_turn_snapshot("turn-2", 2, None).await;

    // Stack should have 2 snapshots
    assert_eq!(mgr.last_snapshot_turn().await, Some(2));
    let checkpoints = mgr.list_checkpoints().await;
    assert_eq!(checkpoints.len(), 2);

    // Turn 3 — should trim turn 1
    mgr.start_turn_snapshot("turn-3", 3, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, b"v3").await.unwrap();
    mgr.finalize_turn_snapshot("turn-3", 3, None).await;

    // Stack should still have 2 snapshots (turns 2 and 3)
    let checkpoints = mgr.list_checkpoints().await;
    assert_eq!(checkpoints.len(), 2);
    assert_eq!(checkpoints[0].turn_number, 2);
    assert_eq!(checkpoints[1].turn_number, 3);

    // Turn 1 backup entries should be cleaned up
    let turn1_entries = mgr.backup_store().entries_for_turn("turn-1").await;
    assert!(turn1_entries.is_empty());
}

#[tokio::test]
async fn test_dry_run_diff_stats_modified_file() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, "line1\nline2\nline3\n")
        .await
        .unwrap();

    // Turn 1: backup, then modify
    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, "line1\nchanged\nline3\nnew_line\n")
        .await
        .unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Dry run: should detect differences
    let stats = mgr.dry_run_diff_stats(1).await.unwrap();
    assert_eq!(stats.files_changed, 1);
    // "changed" replaces "line2" (+1 -1), "new_line" is added (+1)
    assert!(stats.insertions > 0 || stats.deletions > 0);
}

#[tokio::test]
async fn test_dry_run_diff_stats_new_file() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("new.txt");
    // File doesn't exist yet

    // Turn 1: backup (records non-existence), then create
    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, "hello\nworld\n").await.unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Dry run: rewind would delete the newly created file
    let stats = mgr.dry_run_diff_stats(1).await.unwrap();
    assert_eq!(stats.files_changed, 1);
    assert_eq!(stats.deletions, 2); // 2 lines would be removed
    assert_eq!(stats.insertions, 0);
}

#[tokio::test]
async fn test_dry_run_diff_stats_no_changes() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, "original\n").await.unwrap();

    // Turn 1: backup, then write back the same content
    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, "original\n").await.unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Dry run: no changes (file matches backup)
    let stats = mgr.dry_run_diff_stats(1).await.unwrap();
    assert_eq!(stats.files_changed, 0);
    assert_eq!(stats.insertions, 0);
    assert_eq!(stats.deletions, 0);
}

#[tokio::test]
async fn test_rewind_with_mode_code_only() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"original").await.unwrap();

    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, b"modified").await.unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Rewind with CodeOnly mode
    let result = mgr
        .rewind_to_turn_with_mode(Some(1), RewindMode::CodeOnly)
        .await
        .unwrap();
    assert_eq!(result.rewound_turn, 1);
    assert_eq!(result.mode, RewindMode::CodeOnly);
    assert_eq!(result.restored_files.len(), 1);

    // File should be restored
    let content = tokio::fs::read_to_string(&file).await.unwrap();
    assert_eq!(content, "original");
}

#[tokio::test]
async fn test_rewind_with_mode_conversation_only() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"original").await.unwrap();

    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, b"modified").await.unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Rewind with ConversationOnly — files should NOT be restored
    let result = mgr
        .rewind_to_turn_with_mode(Some(1), RewindMode::ConversationOnly)
        .await
        .unwrap();
    assert_eq!(result.rewound_turn, 1);
    assert_eq!(result.mode, RewindMode::ConversationOnly);
    assert!(result.restored_files.is_empty()); // No file restoration

    // File should still be "modified"
    let content = tokio::fs::read_to_string(&file).await.unwrap();
    assert_eq!(content, "modified");
}

#[tokio::test]
async fn test_vecdeque_trimming_many_snapshots() {
    let tmp = TempDir::new().unwrap();
    // Limit to 5 snapshots to test VecDeque trimming
    let mgr = create_test_manager_with_limit(&tmp, false, 5).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"v0").await.unwrap();

    // Create 10 turns — only the last 5 should remain
    for i in 1..=10 {
        let turn_id = format!("turn-{i}");
        mgr.start_turn_snapshot(&turn_id, i, false).await;
        mgr.backup_store()
            .backup_before_modify(&file)
            .await
            .unwrap();
        tokio::fs::write(&file, format!("v{i}").as_bytes())
            .await
            .unwrap();
        mgr.finalize_turn_snapshot(&turn_id, i, None).await;
    }

    let checkpoints = mgr.list_checkpoints().await;
    assert_eq!(checkpoints.len(), 5);
    assert_eq!(checkpoints[0].turn_number, 6);
    assert_eq!(checkpoints[4].turn_number, 10);

    // Turns 1-5 should have been cleaned up
    for i in 1..=5 {
        let entries = mgr
            .backup_store()
            .entries_for_turn(&format!("turn-{i}"))
            .await;
        assert!(entries.is_empty(), "turn-{i} should be cleaned up");
    }
}

#[tokio::test]
async fn test_dry_run_diff_stats_binary_file() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("image.bin");
    // Write binary content (non-UTF-8 bytes)
    let binary_v1: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF, 0xFE];
    tokio::fs::write(&file, &binary_v1).await.unwrap();

    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    // Write different binary content
    let binary_v2: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xAA, 0xBB, 0xCC];
    tokio::fs::write(&file, &binary_v2).await.unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Dry run: binary file should be counted as changed (different size)
    let stats = mgr.dry_run_diff_stats(1).await.unwrap();
    assert_eq!(stats.files_changed, 1);
    // Binary files have no line-level diff
    assert_eq!(stats.insertions, 0);
    assert_eq!(stats.deletions, 0);
}

#[tokio::test]
async fn test_rewind_to_specific_turn_multi() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"v0").await.unwrap();

    // Create 3 turns
    for i in 1..=3 {
        let turn_id = format!("turn-{i}");
        mgr.start_turn_snapshot(&turn_id, i, false).await;
        mgr.backup_store()
            .backup_before_modify(&file)
            .await
            .unwrap();
        tokio::fs::write(&file, format!("v{i}").as_bytes())
            .await
            .unwrap();
        mgr.finalize_turn_snapshot(&turn_id, i, None).await;
    }

    assert_eq!(mgr.list_checkpoints().await.len(), 3);

    // Rewind to turn 2 — should remove turns 2 and 3, restore to pre-turn-2 state
    let result = mgr
        .rewind_to_turn_with_mode(Some(2), RewindMode::CodeAndConversation)
        .await
        .unwrap();
    assert_eq!(result.rewound_turn, 2);
    let content = tokio::fs::read_to_string(&file).await.unwrap();
    assert_eq!(content, "v1"); // Restored to state before turn 2

    // Only turn 1 should remain
    let checkpoints = mgr.list_checkpoints().await;
    assert_eq!(checkpoints.len(), 1);
    assert_eq!(checkpoints[0].turn_number, 1);
}

#[tokio::test]
async fn test_restore_skips_unchanged_file() {
    let tmp = TempDir::new().unwrap();
    let mgr = create_test_manager(&tmp, false).await;

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"original").await.unwrap();

    // Turn 1: backup, modify, then manually restore to original
    mgr.start_turn_snapshot("turn-1", 1, false).await;
    mgr.backup_store()
        .backup_before_modify(&file)
        .await
        .unwrap();
    tokio::fs::write(&file, b"modified").await.unwrap();
    mgr.finalize_turn_snapshot("turn-1", 1, None).await;

    // Manually revert the file before rewinding
    tokio::fs::write(&file, b"original").await.unwrap();

    // Rewind should succeed but skip the actual write (file_needs_restore = false)
    let result = mgr.rewind_last_turn().await.unwrap();
    assert_eq!(result.rewound_turn, 1);
    // The file content should still be "original" (no unnecessary write)
    let content = tokio::fs::read_to_string(&file).await.unwrap();
    assert_eq!(content, "original");
}
