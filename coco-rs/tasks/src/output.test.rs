use super::*;

#[test]
fn test_output_path() {
    let path = get_task_output_path(Path::new("/tmp/sessions"), "session-123", "task-456");
    assert!(path.to_str().unwrap().contains("task-456.output"));
}

#[test]
fn test_write_and_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.output");
    write_task_output(&path, "hello world").unwrap();
    let content = read_task_output(&path).unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn test_append() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("append.output");
    append_task_output(&path, "line 1\n").unwrap();
    append_task_output(&path, "line 2\n").unwrap();
    let content = read_task_output(&path).unwrap();
    assert_eq!(content, "line 1\nline 2\n");
}
