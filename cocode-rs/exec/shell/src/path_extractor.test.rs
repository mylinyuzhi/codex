use super::*;

#[test]
fn test_truncate_for_extraction_short() {
    let output = "hello world";
    let truncated = truncate_for_extraction(output);
    assert_eq!(truncated, "hello world");
}

#[test]
fn test_truncate_for_extraction_long() {
    let output = "x".repeat(3000);
    let truncated = truncate_for_extraction(&output);
    assert_eq!(truncated.len(), MAX_EXTRACTION_OUTPUT_CHARS);
}

#[test]
fn test_truncate_for_extraction_utf8_boundary() {
    // Create a string with multi-byte UTF-8 characters
    let output = "中".repeat(1000); // Each 中 is 3 bytes
    let truncated = truncate_for_extraction(&output);
    // Should not panic and should be valid UTF-8
    assert!(truncated.is_ascii() || !truncated.is_empty());
}

#[test]
fn test_path_extraction_result_new() {
    let paths = vec![PathBuf::from("/file1.txt")];
    let result = PathExtractionResult::new(paths.clone(), 50);
    assert_eq!(result.paths, paths);
    assert_eq!(result.extraction_ms, 50);
}

#[test]
fn test_path_extraction_result_empty() {
    let result = PathExtractionResult::empty();
    assert!(result.paths.is_empty());
    assert_eq!(result.extraction_ms, 0);
}

#[tokio::test]
async fn test_noop_extractor_returns_empty() {
    let extractor = NoOpExtractor;
    let result = extractor
        .extract_paths("ls", "file1.txt", Path::new("/tmp"))
        .await
        .expect("should not fail");
    assert!(result.paths.is_empty());
}

#[test]
fn test_noop_extractor_is_not_enabled() {
    let extractor = NoOpExtractor;
    assert!(!extractor.is_enabled());
}

#[test]
fn test_filter_existing_files_absolute() {
    // Test with a known existing file
    let paths = vec![
        PathBuf::from("/etc/passwd"),           // Should exist on Unix
        PathBuf::from("/nonexistent_file_xyz"), // Should not exist
    ];

    let filtered = filter_existing_files(paths, Path::new("/tmp"));

    #[cfg(unix)]
    {
        // On Unix, /etc/passwd should exist
        assert!(filtered.iter().any(|p| p == Path::new("/etc/passwd")));
        // Nonexistent file should be filtered out
        assert!(
            !filtered
                .iter()
                .any(|p| p == Path::new("/nonexistent_file_xyz"))
        );
    }
}

#[test]
fn test_filter_existing_files_relative() {
    // Create a temp file to test with
    let tmp = tempfile::tempdir().expect("create temp dir");
    let test_file = tmp.path().join("test.txt");
    std::fs::write(&test_file, "test").expect("write test file");

    let paths = vec![PathBuf::from("test.txt"), PathBuf::from("nonexistent.txt")];

    let filtered = filter_existing_files(paths, tmp.path());

    // test.txt should be found (resolved relative to cwd)
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0], test_file);
}
