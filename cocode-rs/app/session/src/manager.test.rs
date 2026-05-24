use super::*;
use crate::persistence::save_session_to_file;
use crate::session::Session;
use cocode_protocol::ModelSpec;
use cocode_protocol::RoleSelection;
use tempfile::TempDir;

#[test]
fn test_session_manager_new() {
    let manager = SessionManager::new();
    assert_eq!(manager.active_count(), 0);
}

#[test]
fn test_session_manager_with_storage_dir() {
    let manager = SessionManager::with_storage_dir(PathBuf::from("/custom/path"));
    assert_eq!(manager.storage_dir, PathBuf::from("/custom/path"));
}

#[test]
fn test_list_active_empty() {
    let manager = SessionManager::new();
    let active = manager.list_active();
    assert!(active.is_empty());
}

#[tokio::test]
async fn test_list_persisted_empty_dir() {
    let temp_dir = TempDir::new().unwrap();
    let manager = SessionManager::with_storage_dir(temp_dir.path().to_path_buf());
    let persisted = manager.list_persisted().await.unwrap();
    assert!(persisted.is_empty());
}

#[tokio::test]
async fn test_list_persisted_nonexistent_dir() {
    let manager = SessionManager::with_storage_dir(PathBuf::from("/nonexistent/path"));
    let persisted = manager.list_persisted().await.unwrap();
    assert!(persisted.is_empty());
}

#[tokio::test]
async fn test_cleanup_expired_sessions() {
    let temp_dir = TempDir::new().unwrap();
    let storage_dir = temp_dir.path().to_path_buf();
    let history = cocode_message::MessageHistory::new();

    // Create an "old" session (expired) — manually set last_activity_at to 60 days ago
    let selection_old = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let mut old_session = Session::new(PathBuf::from("/old"), selection_old);
    old_session.last_activity_at = chrono::Utc::now() - chrono::Duration::days(60);
    let old_id = old_session.id.clone();
    let old_path = storage_dir.join(format!("{old_id}.json"));
    save_session_to_file(&old_session, &history, Vec::new(), &old_path)
        .await
        .unwrap();

    // Create a backup directory for the old session
    let old_backup_dir = storage_dir.join(&old_id);
    tokio::fs::create_dir_all(&old_backup_dir).await.unwrap();
    tokio::fs::write(old_backup_dir.join("blob"), b"data")
        .await
        .unwrap();

    // Create a "recent" session (not expired)
    let selection_new = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let recent_session = Session::new(PathBuf::from("/recent"), selection_new);
    let recent_id = recent_session.id.clone();
    let recent_path = storage_dir.join(format!("{recent_id}.json"));
    save_session_to_file(&recent_session, &history, Vec::new(), &recent_path)
        .await
        .unwrap();

    // Run cleanup with 30-day retention
    let mgr = SessionManager::with_storage_dir(storage_dir);
    let cleaned = mgr.cleanup_expired_sessions(30).await.unwrap();

    assert_eq!(cleaned, 1);

    // Old session file and backup dir should be deleted
    assert!(!old_path.exists());
    assert!(!old_backup_dir.exists());

    // Recent session should still exist
    assert!(recent_path.exists());
}

#[tokio::test]
async fn test_cleanup_nonexistent_dir() {
    let mgr = SessionManager::with_storage_dir(PathBuf::from("/nonexistent/cleanup/path"));
    let cleaned = mgr.cleanup_expired_sessions(30).await.unwrap();
    assert_eq!(cleaned, 0);
}
