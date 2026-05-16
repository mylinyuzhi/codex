use crate::SessionManager;
use crate::storage::TranscriptEntry;
use crate::storage::TranscriptStore;
use std::path::Path;
use std::sync::Arc;

/// Synthetic cwd every test in this module uses. Tests write JSONL
/// fixtures under the slug coco-paths produces for this path so the
/// `SessionManager` global scan can locate them.
const TEST_CWD: &str = "/test-cwd";

fn project_paths(memory_base: &Path) -> Arc<coco_paths::ProjectPaths> {
    Arc::new(coco_paths::ProjectPaths::new(
        memory_base.to_path_buf(),
        Path::new(TEST_CWD),
    ))
}

fn seed_transcript(memory_base: &Path, sid: &str) -> Arc<coco_paths::ProjectPaths> {
    let paths = project_paths(memory_base);
    let store = TranscriptStore::new(paths.clone());
    let entry = TranscriptEntry {
        entry_type: "user".to_string(),
        uuid: format!("{sid}-u1"),
        parent_uuid: None,
        session_id: sid.to_string(),
        cwd: TEST_CWD.to_string(),
        timestamp: "2025-01-15T10:00:00Z".to_string(),
        version: Some("1.0.0".to_string()),
        git_branch: None,
        is_sidechain: false,
        message: Some(serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "hi"}],
        })),
        usage: None,
        model: None,
        cost_usd: None,
        extra: serde_json::Map::new(),
    };
    store.append_message(sid, &entry).unwrap();
    paths
}

#[test]
fn create_returns_in_memory_session_without_writing_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().to_path_buf());
    let session = mgr.create("test-model", Path::new("/tmp")).unwrap();
    assert_eq!(session.model, "test-model");
    assert!(!session.id.is_empty());
    // TS parity: `create` is in-memory only. Nothing on disk yet.
    assert!(mgr.load(&session.id).is_err());
}

#[test]
fn load_derives_session_from_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().to_path_buf());
    seed_transcript(dir.path(), "sess-alpha");
    let loaded = mgr.load("sess-alpha").unwrap();
    assert_eq!(loaded.id, "sess-alpha");
    assert_eq!(loaded.working_dir, std::path::PathBuf::from(TEST_CWD));
}

#[test]
fn list_walks_every_project() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().to_path_buf());
    seed_transcript(dir.path(), "sess-a");
    seed_transcript(dir.path(), "sess-b");
    let list = mgr.list().unwrap();
    assert_eq!(list.len(), 2);
}

#[test]
fn resume_is_equivalent_to_load() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().to_path_buf());
    seed_transcript(dir.path(), "sess-r");
    let resumed = mgr.resume("sess-r").unwrap();
    assert_eq!(resumed.id, "sess-r");
}

#[test]
fn delete_removes_jsonl_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().to_path_buf());
    let paths = seed_transcript(dir.path(), "sess-d");
    assert!(paths.transcript("sess-d").exists());
    mgr.delete("sess-d").unwrap();
    assert!(!paths.transcript("sess-d").exists());
    assert!(mgr.load("sess-d").is_err());
}

#[test]
fn most_recent_returns_newest_session() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().to_path_buf());
    assert!(mgr.most_recent().unwrap().is_none());
    seed_transcript(dir.path(), "sess-x");
    assert!(mgr.most_recent().unwrap().is_some());
}

#[test]
fn set_title_appends_custom_title_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().to_path_buf());
    let paths = seed_transcript(dir.path(), "sess-t");
    let session = mgr.set_title("sess-t", "my session").unwrap();
    assert_eq!(session.title.as_deref(), Some("my session"));

    // Re-derive and confirm the title is read back from the JSONL.
    let loaded = mgr.load("sess-t").unwrap();
    assert_eq!(loaded.title.as_deref(), Some("my session"));
    // Sanity: the metadata sits in the same transcript, not a sidecar.
    let raw = std::fs::read_to_string(paths.transcript("sess-t")).unwrap();
    assert!(
        raw.contains("\"custom-title\""),
        "expected metadata entry in transcript: {raw}",
    );
}

#[test]
fn cleanup_older_than_unlinks_stale_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().to_path_buf());
    let paths = seed_transcript(dir.path(), "old");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let removed = mgr.cleanup_older_than(std::time::Duration::ZERO).unwrap();
    assert_eq!(removed, 1);
    assert!(!paths.transcript("old").exists());
}
