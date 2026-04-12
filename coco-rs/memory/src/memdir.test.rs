use super::*;

#[test]
fn test_generate_filename() {
    let dir = MemoryDir::new(Path::new("/tmp/test"));
    assert_eq!(dir.generate_filename("user role"), "user_role.md");
    assert_eq!(dir.generate_filename("feedback-style"), "feedback-style.md");
}

#[test]
fn test_file_path() {
    let dir = MemoryDir::new(Path::new("/project"));
    let path = dir.file_path("test.md");
    assert_eq!(path, PathBuf::from("/project/.claude/memory/test.md"));
}
