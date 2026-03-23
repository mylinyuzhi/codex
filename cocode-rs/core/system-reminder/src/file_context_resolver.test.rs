use super::*;

#[test]
fn test_resolved_file_from_mention() {
    let cwd = Path::new("/project");

    // Absolute path
    let resolved = ResolvedFile::from_mention("/abs/path.txt", cwd);
    assert_eq!(resolved.path, PathBuf::from("/abs/path.txt"));

    // Relative path
    let resolved = ResolvedFile::from_mention("src/main.rs", cwd);
    assert_eq!(resolved.path, PathBuf::from("/project/src/main.rs"));
}

#[test]
fn test_resolved_file_with_line_range() {
    let cwd = Path::new("/project");
    let resolved = ResolvedFile::from_mention("test.rs", cwd).with_line_range(Some(10), Some(20));

    assert!(resolved.is_partial);
    assert_eq!(resolved.line_start, Some(10));
    assert_eq!(resolved.line_end, Some(20));
}

#[test]
fn test_file_read_config_default() {
    let config = FileReadConfig::default();
    assert_eq!(config.max_file_size, 10 * 1024 * 1024);
    assert_eq!(config.max_lines, 2000);
    assert_eq!(config.max_line_length, 2000);
}

#[test]
fn test_deduplicate_mentions() {
    let mentions = [
        PathBuf::from("/project/src/main.rs"),
        PathBuf::from("/project/src/lib.rs"),
        PathBuf::from("/project/src/main.rs"), // duplicate
    ];

    let deduped = deduplicate_mentions(mentions.iter());
    assert_eq!(deduped.len(), 2);
}

#[test]
fn test_has_line_range() {
    assert!(has_line_range(Some(1), Some(10)));
    assert!(has_line_range(Some(1), None));
    assert!(has_line_range(None, Some(10)));
    assert!(!has_line_range(None, None));
}
