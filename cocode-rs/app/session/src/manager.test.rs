use super::*;
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
