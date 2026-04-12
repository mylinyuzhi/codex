use crate::SessionManager;

#[test]
fn test_create_session() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().join("sessions"));
    let session = mgr
        .create("test-model", std::path::Path::new("/tmp"))
        .unwrap();
    assert_eq!(session.model, "test-model");
    assert!(!session.id.is_empty());
}

#[test]
fn test_load_session() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().join("sessions"));
    let session = mgr.create("opus", std::path::Path::new("/tmp")).unwrap();
    let loaded = mgr.load(&session.id).unwrap();
    assert_eq!(loaded.model, "opus");
}

#[test]
fn test_list_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().join("sessions"));
    mgr.create("model-a", std::path::Path::new("/tmp")).unwrap();
    mgr.create("model-b", std::path::Path::new("/tmp")).unwrap();
    let list = mgr.list().unwrap();
    assert_eq!(list.len(), 2);
}

#[test]
fn test_resume_session() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().join("sessions"));
    let session = mgr.create("opus", std::path::Path::new("/tmp")).unwrap();
    assert!(session.updated_at.is_none());

    let resumed = mgr.resume(&session.id).unwrap();
    assert!(resumed.updated_at.is_some());
}

#[test]
fn test_delete_session() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().join("sessions"));
    let session = mgr.create("opus", std::path::Path::new("/tmp")).unwrap();
    mgr.delete(&session.id).unwrap();
    assert!(mgr.load(&session.id).is_err());
}

#[test]
fn test_most_recent() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().join("sessions"));
    assert!(mgr.most_recent().unwrap().is_none());
    mgr.create("opus", std::path::Path::new("/tmp")).unwrap();
    assert!(mgr.most_recent().unwrap().is_some());
}

#[test]
fn test_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = SessionManager::new(dir.path().join("sessions"));
    for _ in 0..5 {
        mgr.create("m", std::path::Path::new("/tmp")).unwrap();
    }
    let removed = mgr.cleanup(/*keep*/ 2).unwrap();
    assert_eq!(removed, 3);
    assert_eq!(mgr.list().unwrap().len(), 2);
}
