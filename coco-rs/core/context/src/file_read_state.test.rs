use std::path::PathBuf;

use super::*;

#[test]
fn test_set_and_get() {
    let mut state = FileReadState::new();
    let path = PathBuf::from("/tmp/test.rs");
    state.set(
        path.clone(),
        FileReadEntry {
            content: "fn main() {}".to_string(),
            mtime_ms: 1000,
            offset: None,
            limit: None,
        },
    );
    let entry = state.get(&path).expect("should find entry");
    assert_eq!(entry.content, "fn main() {}");
    assert_eq!(entry.mtime_ms, 1000);
    assert_eq!(state.len(), 1);
}

#[test]
fn test_peek_does_not_update_lru() {
    let mut state = FileReadState::new();
    let path = PathBuf::from("/tmp/test.rs");
    state.set(
        path.clone(),
        FileReadEntry {
            content: "hello".to_string(),
            mtime_ms: 1000,
            offset: None,
            limit: None,
        },
    );
    // peek should return the entry
    assert!(state.peek(&path).is_some());
    // LRU order should still have path at end (unchanged)
    assert_eq!(state.access_order.last(), Some(&path));
}

#[test]
fn test_update_after_edit() {
    let mut state = FileReadState::new();
    let path = PathBuf::from("/tmp/test.rs");
    state.set(
        path.clone(),
        FileReadEntry {
            content: "old".to_string(),
            mtime_ms: 1000,
            offset: Some(5),
            limit: Some(10),
        },
    );
    state.update_after_edit(&path, "new content".to_string(), 2000);
    let entry = state.get(&path).expect("should find entry");
    assert_eq!(entry.content, "new content");
    assert_eq!(entry.mtime_ms, 2000);
    // offset/limit should be cleared after edit
    assert_eq!(entry.offset, None);
    assert_eq!(entry.limit, None);
}

#[test]
fn test_invalidate() {
    let mut state = FileReadState::new();
    let path = PathBuf::from("/tmp/test.rs");
    state.set(
        path.clone(),
        FileReadEntry {
            content: "x".to_string(),
            mtime_ms: 1000,
            offset: None,
            limit: None,
        },
    );
    state.invalidate(&path);
    assert!(state.get(&path).is_none());
    assert_eq!(state.len(), 0);
}

#[test]
fn test_lru_eviction() {
    let mut state = FileReadState::new();
    // Fill to capacity
    for i in 0..100 {
        state.set(
            PathBuf::from(format!("/tmp/file{i}.rs")),
            FileReadEntry {
                content: format!("content {i}"),
                mtime_ms: i as i64,
                offset: None,
                limit: None,
            },
        );
    }
    assert_eq!(state.len(), 100);

    // Adding one more should evict the oldest (file0)
    state.set(
        PathBuf::from("/tmp/overflow.rs"),
        FileReadEntry {
            content: "overflow".to_string(),
            mtime_ms: 999,
            offset: None,
            limit: None,
        },
    );
    assert_eq!(state.len(), 100);
    assert!(state.peek(&PathBuf::from("/tmp/file0.rs")).is_none());
    assert!(state.peek(&PathBuf::from("/tmp/overflow.rs")).is_some());
}

#[test]
fn test_iter_entries() {
    let mut state = FileReadState::new();
    state.set(
        PathBuf::from("/a.rs"),
        FileReadEntry {
            content: "a".to_string(),
            mtime_ms: 1,
            offset: None,
            limit: None,
        },
    );
    state.set(
        PathBuf::from("/b.rs"),
        FileReadEntry {
            content: "b".to_string(),
            mtime_ms: 2,
            offset: None,
            limit: None,
        },
    );
    let entries: Vec<_> = state.iter_entries().collect();
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn test_is_unchanged_missing_file() {
    let state = FileReadState::new();
    // File not in cache → false
    assert!(
        !state
            .is_unchanged(std::path::Path::new("/nonexistent"))
            .await
    );
}

#[tokio::test]
async fn test_is_unchanged_with_tempfile() {
    use std::io::Write;
    let mut state = FileReadState::new();
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    {
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"hello").unwrap();
    }
    let mtime = file_mtime_ms(&file_path).await.unwrap();
    state.set(
        file_path.clone(),
        FileReadEntry {
            content: "hello".to_string(),
            mtime_ms: mtime,
            offset: None,
            limit: None,
        },
    );
    // Should be unchanged (mtime matches)
    assert!(state.is_unchanged(&file_path).await);

    // Modify the file
    std::thread::sleep(std::time::Duration::from_millis(50));
    {
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"modified").unwrap();
    }
    // Now should be changed
    assert!(!state.is_unchanged(&file_path).await);
}

#[test]
fn test_snapshot_by_recency_ordering() {
    let mut state = FileReadState::new();
    let make = |name: &str, mtime: i64| {
        (
            PathBuf::from(name),
            FileReadEntry {
                content: name.to_string(),
                mtime_ms: mtime,
                offset: None,
                limit: None,
            },
        )
    };
    let (pa, ea) = make("/a.rs", 1);
    let (pb, eb) = make("/b.rs", 2);
    let (pc, ec) = make("/c.rs", 3);

    state.set(pa.clone(), ea);
    state.set(pb.clone(), eb);
    state.set(pc.clone(), ec);
    // Touch /a.rs so it becomes most recent
    let _ = state.get(&pa);

    let snap = state.snapshot_by_recency();
    assert_eq!(snap.len(), 3);
    // LRU order: b (oldest), c, a (most recent)
    assert_eq!(snap[0].0, pb);
    assert_eq!(snap[1].0, pc);
    assert_eq!(snap[2].0, pa);
}

#[test]
fn test_snapshot_empty() {
    let state = FileReadState::new();
    assert!(state.snapshot_by_recency().is_empty());
}
