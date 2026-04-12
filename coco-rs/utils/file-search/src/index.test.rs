use super::*;

#[test]
fn test_extract_directories_basic() {
    let files = vec![
        "src/main.rs".to_string(),
        "src/lib.rs".to_string(),
        "src/utils/mod.rs".to_string(),
        "Cargo.toml".to_string(),
    ];

    let dirs = extract_directories(&files);

    assert!(dirs.contains(&"src/".to_string()));
    assert!(dirs.contains(&"src/utils/".to_string()));
}

#[test]
fn test_extract_directories_nested() {
    let files = vec![
        "src/components/Button.tsx".to_string(),
        "src/components/Input.tsx".to_string(),
        "src/utils/helpers.ts".to_string(),
    ];

    let dirs = extract_directories(&files);

    assert!(dirs.contains(&"src/".to_string()));
    assert!(dirs.contains(&"src/components/".to_string()));
    assert!(dirs.contains(&"src/utils/".to_string()));
}

#[test]
fn test_file_suggestion_new() {
    let suggestion = FileSuggestion::new("src/main.rs".to_string(), 100, vec![0, 4, 5]);

    assert_eq!(suggestion.path, "src/main.rs");
    assert_eq!(suggestion.score, 100);
    assert_eq!(suggestion.match_indices, vec![0, 4, 5]);
    assert!(!suggestion.is_directory);
}

#[test]
fn test_file_suggestion_directory() {
    let suggestion = FileSuggestion::directory("src".to_string());

    assert_eq!(suggestion.path, "src");
    assert_eq!(suggestion.display_text, "src/");
    assert!(suggestion.is_directory);
}

#[test]
fn test_file_index_cache_validity() {
    let index = FileIndex::new("/tmp");
    assert!(!index.is_cache_valid());
}

#[tokio::test]
async fn test_discovery_result_default() {
    let result = DiscoveryResult::default();
    assert!(result.files.is_empty());
    assert!(result.directories.is_empty());
}
