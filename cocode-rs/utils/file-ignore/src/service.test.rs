use super::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_with_defaults() {
    let service = IgnoreService::with_defaults();
    assert!(service.config().respect_gitignore);
    assert!(service.config().respect_ignore);
}

#[test]
fn test_create_walk_builder() {
    let temp = tempdir().expect("create temp dir");
    let service = IgnoreService::with_defaults();
    let _builder = service.create_walk_builder(temp.path());
    // Verify it doesn't panic
}

#[test]
fn test_respects_gitignore() {
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();

    // Create test files
    fs::write(dir.join("keep.rs"), "code").expect("write");
    fs::write(dir.join("ignored.log"), "log").expect("write");
    fs::write(dir.join(".gitignore"), "*.log").expect("write");

    let service = IgnoreService::with_defaults();
    let walker = service.create_walk_builder(dir);

    let files: Vec<_> = walker
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    assert!(files.contains(&"keep.rs".to_string()));
    // .gitignore itself is a dotfile, and we exclude hidden files by default
    assert!(!files.contains(&"ignored.log".to_string()));
}

#[test]
fn test_respects_ignore() {
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();

    // Create test files
    fs::write(dir.join("keep.rs"), "code").expect("write");
    fs::write(dir.join("secret.env"), "secrets").expect("write");
    fs::write(dir.join(".ignore"), "*.env").expect("write");

    let service = IgnoreService::with_defaults();
    let walker = service.create_walk_builder(dir);

    let files: Vec<_> = walker
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    assert!(files.contains(&"keep.rs".to_string()));
    assert!(!files.contains(&"secret.env".to_string()));
}

#[test]
fn test_custom_excludes() {
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();

    fs::write(dir.join("keep.rs"), "code").expect("write");
    fs::write(dir.join("temp.tmp"), "temp").expect("write");

    let config = IgnoreConfig::default().with_excludes(vec!["*.tmp".to_string()]);
    let service = IgnoreService::new(config);
    let walker = service.create_walk_builder(dir);

    let files: Vec<_> = walker
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    assert!(files.contains(&"keep.rs".to_string()));
    assert!(!files.contains(&"temp.tmp".to_string()));
}

#[test]
fn test_get_core_patterns() {
    let patterns = IgnoreService::get_core_patterns();
    assert!(patterns.contains(&"**/node_modules/**"));
    assert!(patterns.contains(&"**/.git/**"));
}

#[test]
fn test_get_default_excludes() {
    let excludes = IgnoreService::get_default_excludes();
    assert!(excludes.len() > 10);
    assert!(excludes.contains(&"**/*.exe"));
    assert!(excludes.contains(&"**/.DS_Store"));
}

#[test]
fn test_include_hidden_files() {
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();

    fs::write(dir.join("visible.rs"), "code").expect("write");
    fs::write(dir.join(".hidden"), "hidden").expect("write");

    // Default: exclude hidden
    let service = IgnoreService::with_defaults();
    let walker = service.create_walk_builder(dir);
    let files: Vec<_> = walker
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(files.contains(&"visible.rs".to_string()));
    assert!(!files.contains(&".hidden".to_string()));

    // With hidden included
    let config = IgnoreConfig::default().with_hidden(true);
    let service = IgnoreService::new(config);
    let walker = service.create_walk_builder(dir);
    let files: Vec<_> = walker
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(files.contains(&"visible.rs".to_string()));
    assert!(files.contains(&".hidden".to_string()));
}

#[test]
fn test_find_ignore_files_single() {
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();

    // Create .ignore file
    fs::write(dir.join(".ignore"), "*.log").expect("write file");

    let files = find_ignore_files(dir);
    assert!(!files.is_empty());
    assert!(files.iter().any(|f| f.ends_with(".ignore")));
}

#[test]
fn test_find_ignore_files_nested() {
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();

    // Create nested directory structure
    fs::create_dir_all(dir.join("src/nested")).expect("create dirs");

    // Root-level .ignore
    fs::write(dir.join(".ignore"), "*.log").expect("write root ignore");

    // Nested .ignore in src/
    fs::write(dir.join("src/.ignore"), "*.tmp").expect("write src ignore");

    // Deeply nested .ignore
    fs::write(dir.join("src/nested/.ignore"), "*.bak").expect("write nested ignore");

    let files = find_ignore_files(dir);

    // Should find all 3 ignore files
    assert!(files.len() >= 3);
    assert!(
        files
            .iter()
            .any(|f| { f.ends_with(".ignore") && f.parent().map(|p| p == dir).unwrap_or(false) })
    );
    assert!(files.iter().any(|f| {
        f.ends_with(".ignore")
            && f.parent()
                .and_then(|p| p.file_name())
                .map(|n| n == "src")
                .unwrap_or(false)
    }));
    assert!(files.iter().any(|f| {
        f.ends_with(".ignore")
            && f.parent()
                .and_then(|p| p.file_name())
                .map(|n| n == "nested")
                .unwrap_or(false)
    }));
}

#[test]
fn test_find_ignore_files_no_duplicates() {
    let temp = tempdir().expect("create temp dir");
    let dir = temp.path();

    // Create .ignore file at root
    fs::write(dir.join(".ignore"), "*.log").expect("write file");

    let files = find_ignore_files(dir);

    // Count occurrences of root .ignore
    let root_count = files
        .iter()
        .filter(|f| f.parent().map(|p| p == dir).unwrap_or(false) && f.ends_with(".ignore"))
        .count();

    assert_eq!(root_count, 1, "Should not have duplicate root ignore file");
}
