use super::*;
use tempfile::TempDir;

#[tokio::test]
async fn test_backup_and_restore_existing_file() {
    let tmp = TempDir::new().unwrap();
    let store = FileBackupStore::with_dir(tmp.path().join("backups"))
        .await
        .unwrap();

    // Create a file
    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"original content").await.unwrap();

    // Backup before modify
    store.set_current_turn("turn-1").await;
    store.backup_before_modify(&file).await.unwrap();

    // Modify file
    tokio::fs::write(&file, b"modified content").await.unwrap();
    assert_eq!(
        tokio::fs::read_to_string(&file).await.unwrap(),
        "modified content"
    );

    // Restore
    let restored = store.restore_turn("turn-1").await.unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(
        tokio::fs::read_to_string(&file).await.unwrap(),
        "original content"
    );
}

#[tokio::test]
async fn test_backup_nonexistent_file_deletes_on_restore() {
    let tmp = TempDir::new().unwrap();
    let store = FileBackupStore::with_dir(tmp.path().join("backups"))
        .await
        .unwrap();

    let file = tmp.path().join("new_file.txt");
    assert!(!file.exists());

    // Backup before create (file doesn't exist yet)
    store.set_current_turn("turn-1").await;
    store.backup_before_modify(&file).await.unwrap();

    // Create file
    tokio::fs::write(&file, b"new content").await.unwrap();
    assert!(file.exists());

    // Restore should delete the file
    let restored = store.restore_turn("turn-1").await.unwrap();
    assert_eq!(restored.len(), 1);
    assert!(!file.exists());
}

#[tokio::test]
async fn test_dedup_same_turn() {
    let tmp = TempDir::new().unwrap();
    let store = FileBackupStore::with_dir(tmp.path().join("backups"))
        .await
        .unwrap();

    let file = tmp.path().join("dup.txt");
    tokio::fs::write(&file, b"content").await.unwrap();

    store.set_current_turn("turn-1").await;
    store.backup_before_modify(&file).await.unwrap();
    store.backup_before_modify(&file).await.unwrap(); // should be deduped

    let entries = store.entries_for_turn("turn-1").await;
    assert_eq!(entries.len(), 1);
}

#[tokio::test]
async fn test_content_dedup_across_files() {
    let tmp = TempDir::new().unwrap();
    let store = FileBackupStore::with_dir(tmp.path().join("backups"))
        .await
        .unwrap();

    let file1 = tmp.path().join("a.txt");
    let file2 = tmp.path().join("b.txt");
    tokio::fs::write(&file1, b"same content").await.unwrap();
    tokio::fs::write(&file2, b"same content").await.unwrap();

    store.set_current_turn("turn-1").await;
    store.backup_before_modify(&file1).await.unwrap();
    store.backup_before_modify(&file2).await.unwrap();

    let entries = store.entries_for_turn("turn-1").await;
    assert_eq!(entries.len(), 2);
    // Both should reference the same backup blob
    assert_eq!(entries[0].backup_filename, entries[1].backup_filename);
}

#[tokio::test]
async fn test_index_persistence() {
    let tmp = TempDir::new().unwrap();
    let backup_dir = tmp.path().join("backups");

    {
        let store = FileBackupStore::with_dir(backup_dir.clone()).await.unwrap();
        let file = tmp.path().join("persist.txt");
        tokio::fs::write(&file, b"data").await.unwrap();

        store.set_current_turn("turn-1").await;
        store.backup_before_modify(&file).await.unwrap();
    }

    // Reload from disk
    let store = FileBackupStore::with_dir(backup_dir).await.unwrap();
    let entries = store.entries_for_turn("turn-1").await;
    assert_eq!(entries.len(), 1);
    assert!(entries[0].existed_before);
}

#[cfg(unix)]
#[tokio::test]
async fn test_backup_preserves_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let store = FileBackupStore::with_dir(tmp.path().join("backups"))
        .await
        .unwrap();

    let file = tmp.path().join("executable.sh");
    tokio::fs::write(&file, b"#!/bin/sh\necho hello")
        .await
        .unwrap();

    // Set executable permission (0o755)
    let perms = std::fs::Permissions::from_mode(0o755);
    tokio::fs::set_permissions(&file, perms).await.unwrap();

    // Backup
    store.set_current_turn("turn-1").await;
    store.backup_before_modify(&file).await.unwrap();

    // Verify file_mode was captured
    let entries = store.entries_for_turn("turn-1").await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file_mode, Some(0o100755));

    // Modify file and change permissions
    tokio::fs::write(&file, b"#!/bin/sh\necho modified")
        .await
        .unwrap();
    let new_perms = std::fs::Permissions::from_mode(0o644);
    tokio::fs::set_permissions(&file, new_perms).await.unwrap();

    // Restore
    let restored = store.restore_turn("turn-1").await.unwrap();
    assert_eq!(restored.len(), 1);

    // Verify content restored
    assert_eq!(
        tokio::fs::read_to_string(&file).await.unwrap(),
        "#!/bin/sh\necho hello"
    );

    // Verify permissions restored
    let meta = tokio::fs::metadata(&file).await.unwrap();
    assert_eq!(meta.permissions().mode(), 0o100755);
}

#[tokio::test]
async fn test_file_needs_restore_size_mismatch() {
    let tmp = TempDir::new().unwrap();
    let store = FileBackupStore::with_dir(tmp.path().join("backups"))
        .await
        .unwrap();

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"short").await.unwrap();

    store.set_current_turn("turn-1").await;
    store.backup_before_modify(&file).await.unwrap();

    // Write content with different length — size tier should catch it
    tokio::fs::write(&file, b"this is much longer content")
        .await
        .unwrap();

    // Restore should detect the size mismatch and restore
    let restored = store.restore_turn("turn-1").await.unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), "short");
}

#[tokio::test]
async fn test_file_needs_restore_hash_match_skips() {
    let tmp = TempDir::new().unwrap();
    let store = FileBackupStore::with_dir(tmp.path().join("backups"))
        .await
        .unwrap();

    let file = tmp.path().join("test.txt");
    tokio::fs::write(&file, b"original").await.unwrap();

    store.set_current_turn("turn-1").await;
    store.backup_before_modify(&file).await.unwrap();

    // Modify, then revert to original content before restore
    tokio::fs::write(&file, b"modified").await.unwrap();
    tokio::fs::write(&file, b"original").await.unwrap();

    // Restore should detect the hash match and skip the write.
    // The file is already "original" so no restore happens → empty list.
    let restored = store.restore_turn("turn-1").await.unwrap();
    assert!(
        restored.is_empty(),
        "Should skip restore when content hash matches"
    );
}
