use super::*;
use cocode_protocol::ToolName;
use std::path::Path;

#[test]
fn test_is_read_state_source_tool() {
    assert!(is_read_state_source_tool(ToolName::Read.as_str()));
    assert!(is_read_state_source_tool("ReadManyFiles"));
    assert!(is_read_state_source_tool(ToolName::Glob.as_str()));
    assert!(is_read_state_source_tool(ToolName::Grep.as_str()));

    assert!(!is_read_state_source_tool(ToolName::Edit.as_str()));
    assert!(!is_read_state_source_tool(ToolName::Write.as_str()));
    assert!(!is_read_state_source_tool(ToolName::Bash.as_str()));
}

#[test]
fn test_is_stronger_kind() {
    use cocode_protocol::FileReadKind;

    assert!(is_stronger_kind(
        &FileReadKind::FullContent,
        &FileReadKind::PartialContent
    ));
    assert!(is_stronger_kind(
        &FileReadKind::FullContent,
        &FileReadKind::MetadataOnly
    ));
    assert!(is_stronger_kind(
        &FileReadKind::PartialContent,
        &FileReadKind::MetadataOnly
    ));

    assert!(!is_stronger_kind(
        &FileReadKind::PartialContent,
        &FileReadKind::FullContent
    ));
    assert!(!is_stronger_kind(
        &FileReadKind::MetadataOnly,
        &FileReadKind::FullContent
    ));
    assert!(!is_stronger_kind(
        &FileReadKind::MetadataOnly,
        &FileReadKind::PartialContent
    ));
}

#[test]
fn test_collect_cleared_read_paths() {
    // With modifier paths - should use them directly
    let modifier_paths = vec![PathBuf::from("/src/main.rs")];
    let paths = collect_cleared_read_paths(ToolName::Read.as_str(), &modifier_paths, None);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], PathBuf::from("/src/main.rs"));

    // Without modifier paths - should fall back
    let paths = collect_cleared_read_paths(ToolName::Read.as_str(), &[], Some("/src/lib.rs"));
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], PathBuf::from("/src/lib.rs"));

    // Non-read tool returns empty
    let paths = collect_cleared_read_paths(ToolName::Edit.as_str(), &modifier_paths, None);
    assert!(paths.is_empty());
}

#[test]
fn test_collect_cleared_read_paths_from_input() {
    // With modifier paths - should use them directly
    let modifier_paths = vec![PathBuf::from("/src/main.rs")];
    let input = serde_json::json!({"file_path": "/other.rs"});
    let paths =
        collect_cleared_read_paths_from_input(ToolName::Read.as_str(), &modifier_paths, &input);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], PathBuf::from("/src/main.rs")); // Uses modifier, not input

    // Without modifier paths - parses input
    let input = serde_json::json!({"file_path": "/src/main.rs"});
    let paths = collect_cleared_read_paths_from_input(ToolName::Read.as_str(), &[], &input);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], PathBuf::from("/src/main.rs"));

    // ReadManyFiles with paths array
    let input = serde_json::json!({"paths": ["/src/a.rs", "/src/b.rs"]});
    let paths = collect_cleared_read_paths_from_input("ReadManyFiles", &[], &input);
    assert_eq!(paths.len(), 2);

    // Non-read tool returns empty
    let input = serde_json::json!({"file_path": "/src/main.rs"});
    let paths = collect_cleared_read_paths_from_input(ToolName::Edit.as_str(), &[], &input);
    assert!(paths.is_empty());
}

#[test]
fn test_normalize_path() {
    // Absolute paths with .. are normalized
    assert_eq!(
        normalize_path("/project/src/../lib/file.rs"),
        PathBuf::from("/project/lib/file.rs")
    );

    // Absolute paths with . are normalized
    assert_eq!(
        normalize_path("/project/./src/./file.rs"),
        PathBuf::from("/project/src/file.rs")
    );

    // Mixed . and ..
    assert_eq!(
        normalize_path("/project/src/lib/.././file.rs"),
        PathBuf::from("/project/src/file.rs")
    );

    // Absolute path without special components stays the same
    assert_eq!(
        normalize_path("/project/src/file.rs"),
        PathBuf::from("/project/src/file.rs")
    );

    // Can't go above root
    let result = normalize_path("/../file.rs");
    // Should either keep the .. or stay at root
    assert!(result == Path::new("/../file.rs") || result == Path::new("/file.rs"));
}
