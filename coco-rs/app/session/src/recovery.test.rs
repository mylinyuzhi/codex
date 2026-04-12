use super::*;

#[test]
fn test_can_resume_nonexistent() {
    assert!(!can_resume_session(Path::new("/nonexistent/path.jsonl")));
}

#[test]
fn test_fork_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source.jsonl");
    let dst = dir.path().join("dest.jsonl");
    std::fs::write(&src, "{\"test\": true}\n").unwrap();
    fork_conversation(&src, &dst).unwrap();
    assert!(dst.exists());
    assert_eq!(
        std::fs::read_to_string(&dst).unwrap(),
        std::fs::read_to_string(&src).unwrap()
    );
}
