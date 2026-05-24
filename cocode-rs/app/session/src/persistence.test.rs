use super::*;
use cocode_protocol::ModelSpec;
use cocode_protocol::RoleSelection;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn test_save_and_load_session() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("test_session.json");

    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let session = Session::new(PathBuf::from("/test"), selection);
    let history = MessageHistory::new();

    // Save
    save_session_to_file(&session, &history, Vec::new(), &path)
        .await
        .unwrap();

    assert!(session_exists(&path).await);

    // Load
    let (loaded_session, _loaded_history, loaded_snapshots) =
        load_session_from_file(&path).await.unwrap();

    assert_eq!(loaded_session.id, session.id);
    assert_eq!(loaded_session.model(), session.model());
    assert_eq!(loaded_session.provider(), session.provider());
    assert!(loaded_snapshots.is_empty());
}

#[tokio::test]
async fn test_delete_session_file() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("to_delete.json");

    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let session = Session::new(PathBuf::from("/test"), selection);
    let history = MessageHistory::new();

    save_session_to_file(&session, &history, Vec::new(), &path)
        .await
        .unwrap();
    assert!(session_exists(&path).await);

    delete_session_file(&path).await.unwrap();
    assert!(!session_exists(&path).await);
}

#[test]
fn test_session_file_path() {
    let path = session_file_path("test-id-123");
    assert!(path.to_string_lossy().contains("sessions"));
    assert!(path.to_string_lossy().ends_with("test-id-123.json"));
}

#[test]
fn test_persisted_session_version() {
    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let session = Session::new(PathBuf::from("/test"), selection);
    let history = MessageHistory::new();
    let persisted = PersistedSession::new(session, history, Vec::new());

    assert_eq!(persisted.version, 1);
}

#[tokio::test]
async fn test_save_and_load_with_snapshots() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("session_with_snapshots.json");

    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let session = Session::new(PathBuf::from("/test"), selection);
    let history = MessageHistory::new();

    // Create some test snapshots
    let snapshots = vec![TurnSnapshot {
        turn_id: "turn-1".to_string(),
        turn_number: 1,
        ghost_commit: None,
        file_backups: vec![],
    }];

    // Save with snapshots
    save_session_to_file(&session, &history, snapshots.clone(), &path)
        .await
        .unwrap();

    // Load
    let (loaded_session, _loaded_history, loaded_snapshots) =
        load_session_from_file(&path).await.unwrap();

    assert_eq!(loaded_session.id, session.id);
    assert_eq!(loaded_snapshots.len(), 1);
    assert_eq!(loaded_snapshots[0].turn_id, "turn-1");
    assert_eq!(loaded_snapshots[0].turn_number, 1);
}

#[tokio::test]
async fn test_load_legacy_session_without_snapshots() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("legacy_session.json");

    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let session = Session::new(PathBuf::from("/test"), selection);
    let history = MessageHistory::new();

    // Write a legacy format without snapshots field
    let legacy = serde_json::json!({
        "session": serde_json::to_value(&session).unwrap(),
        "history": serde_json::to_value(&history).unwrap(),
        "version": 1
    });
    tokio::fs::write(&path, serde_json::to_string_pretty(&legacy).unwrap())
        .await
        .unwrap();

    // Load should succeed with empty snapshots (backward compat via #[serde(default)])
    let (loaded_session, _loaded_history, loaded_snapshots) =
        load_session_from_file(&path).await.unwrap();

    assert_eq!(loaded_session.id, session.id);
    assert!(loaded_snapshots.is_empty());
}
