use std::io::Write;

use super::*;

#[tokio::test]
async fn test_no_changes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("stable.txt");
    {
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(b"stable content").unwrap();
    }
    let mtime = file_mtime_ms(&file).await.unwrap();

    let mut state = FileReadState::new();
    state.set(
        file.clone(),
        FileReadEntry {
            content: "stable content".to_string(),
            mtime_ms: mtime,
            offset: None,
            limit: None,
        },
    );

    let changed = detect_changed_files(&mut state).await;
    assert!(changed.is_empty());
}

#[tokio::test]
async fn test_detects_changed_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("changing.txt");
    {
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(b"original").unwrap();
    }
    let mtime = file_mtime_ms(&file).await.unwrap();

    let mut state = FileReadState::new();
    state.set(
        file.clone(),
        FileReadEntry {
            content: "original".to_string(),
            mtime_ms: mtime,
            offset: None,
            limit: None,
        },
    );

    // Modify file after a small delay to ensure mtime changes
    std::thread::sleep(std::time::Duration::from_millis(50));
    {
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(b"modified content").unwrap();
    }

    let changed = detect_changed_files(&mut state).await;
    assert_eq!(changed.len(), 1);
    match &changed[0] {
        Attachment::File(f) => assert_eq!(f.content, "modified content"),
        other => panic!("Expected File, got {other:?}"),
    }

    // State should be updated
    let entry = state.peek(&file).unwrap();
    assert_eq!(entry.content, "modified content");
}

#[tokio::test]
async fn test_skips_partial_reads() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("partial.txt");
    {
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(b"content").unwrap();
    }

    let mut state = FileReadState::new();
    state.set(
        file.clone(),
        FileReadEntry {
            content: "partial".to_string(),
            mtime_ms: 0, // stale mtime → would normally trigger change
            offset: Some(5),
            limit: Some(10),
        },
    );

    // Even though mtime is stale, partial reads should be skipped
    let changed = detect_changed_files(&mut state).await;
    assert!(changed.is_empty());
}
