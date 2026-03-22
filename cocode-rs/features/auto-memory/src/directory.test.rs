use std::path::Path;

use super::*;

#[test]
fn test_project_hash_deterministic() {
    let hash1 = project_hash(Path::new("/home/user/project"));
    let hash2 = project_hash(Path::new("/home/user/project"));
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 12);
}

#[test]
fn test_project_hash_different_paths() {
    let hash1 = project_hash(Path::new("/home/user/project-a"));
    let hash2 = project_hash(Path::new("/home/user/project-b"));
    assert_ne!(hash1, hash2);
}

#[test]
fn test_custom_dir_used_when_set() {
    let dir = get_auto_memory_directory(Path::new("/tmp/test"), Some("/custom/memory"));
    assert_eq!(dir, PathBuf::from("/custom/memory"));
}

#[test]
fn test_default_dir_includes_hash() {
    let dir = get_auto_memory_directory(Path::new("/tmp/test"), None);
    let hash = project_hash(Path::new("/tmp/test"));
    assert!(dir.to_string_lossy().contains(&hash));
    assert!(dir.to_string_lossy().ends_with("memory"));
}

#[tokio::test]
async fn test_ensure_memory_dir_exists() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("memory");
    assert!(!dir.exists());
    ensure_memory_dir_exists(&dir).await.unwrap();
    assert!(dir.exists());
}
