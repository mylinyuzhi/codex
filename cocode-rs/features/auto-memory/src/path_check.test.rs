use tempfile::TempDir;

use super::*;

#[test]
fn test_path_within_memory_dir() {
    let tmp = TempDir::new().unwrap();
    let memory_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&memory_dir).unwrap();
    let file = memory_dir.join("debug.md");
    std::fs::write(&file, "test").unwrap();

    assert!(is_auto_memory_path(&file, &memory_dir));
}

#[test]
fn test_path_outside_memory_dir() {
    let tmp = TempDir::new().unwrap();
    let memory_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&memory_dir).unwrap();
    let file = tmp.path().join("code.rs");
    std::fs::write(&file, "test").unwrap();

    assert!(!is_auto_memory_path(&file, &memory_dir));
}

#[test]
fn test_nested_path_within_memory_dir() {
    let tmp = TempDir::new().unwrap();
    let memory_dir = tmp.path().join("memory");
    let sub_dir = memory_dir.join("team");
    std::fs::create_dir_all(&sub_dir).unwrap();
    let file = sub_dir.join("notes.md");
    std::fs::write(&file, "test").unwrap();

    assert!(is_auto_memory_path(&file, &memory_dir));
}

#[test]
fn test_nonexistent_path_fallback() {
    let memory_dir = std::path::Path::new("/nonexistent/memory");
    let file = std::path::Path::new("/nonexistent/memory/debug.md");

    // Canonical paths won't work, but fallback starts_with should
    assert!(is_auto_memory_path(file, memory_dir));
}

#[test]
fn test_reject_relative_path() {
    let memory_dir = std::path::Path::new("/home/user/.cocode/projects/abc/memory");
    let file = std::path::Path::new("relative/file.md");

    assert!(!is_auto_memory_path(file, memory_dir));
}

#[test]
fn test_reject_path_traversal() {
    let memory_dir = std::path::Path::new("/home/user/.cocode/projects/abc/memory");
    let file = std::path::Path::new("/home/user/.cocode/projects/abc/memory/../../etc/passwd");

    assert!(!is_auto_memory_path(file, memory_dir));
}

#[test]
fn test_reject_null_bytes() {
    let memory_dir = std::path::Path::new("/home/user/.cocode/projects/abc/memory");
    let file = std::path::Path::new("/home/user/.cocode/projects/abc/memory/evil\0.md");

    assert!(!is_auto_memory_path(file, memory_dir));
}
