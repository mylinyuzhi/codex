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
    save_session_to_file(&session, &history, &path)
        .await
        .unwrap();

    assert!(session_exists(&path).await);

    // Load
    let (loaded_session, _loaded_history) = load_session_from_file(&path).await.unwrap();

    assert_eq!(loaded_session.id, session.id);
    assert_eq!(loaded_session.model(), session.model());
    assert_eq!(loaded_session.provider(), session.provider());
}

#[tokio::test]
async fn test_delete_session_file() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("to_delete.json");

    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let session = Session::new(PathBuf::from("/test"), selection);
    let history = MessageHistory::new();

    save_session_to_file(&session, &history, &path)
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
    let persisted = PersistedSession::new(session, history);

    assert_eq!(persisted.version, 1);
}
