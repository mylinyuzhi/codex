use super::*;

#[test]
fn test_is_read_state_source_tool() {
    assert!(is_read_state_source_tool("Read"));
    assert!(is_read_state_source_tool("ReadManyFiles"));
    assert!(is_read_state_source_tool("Glob"));
    assert!(is_read_state_source_tool("Grep"));

    assert!(!is_read_state_source_tool("Edit"));
    assert!(!is_read_state_source_tool("Write"));
    assert!(!is_read_state_source_tool("Bash"));
}

#[test]
fn test_is_full_content_read_tool() {
    assert!(is_full_content_read_tool("Read"));
    assert!(is_full_content_read_tool("ReadManyFiles"));

    assert!(!is_full_content_read_tool("Glob"));
    assert!(!is_full_content_read_tool("Grep"));
    assert!(!is_full_content_read_tool("Edit"));
}

#[test]
fn test_should_skip_tracked_file() {
    // Regular files should not be skipped
    assert!(!should_skip_tracked_file(
        Path::new("/project/src/main.rs"),
        None,
        None,
        &[]
    ));

    // Internal files should be skipped
    assert!(should_skip_tracked_file(
        Path::new("/home/.claude/projects/abc/session-memory/summary.md"),
        None,
        None,
        &[]
    ));
    assert!(should_skip_tracked_file(
        Path::new("/home/.cocode/plans/plan-123.md"),
        None,
        None,
        &[]
    ));
    assert!(should_skip_tracked_file(
        Path::new("/project/MEMORY.md"),
        None,
        None,
        &[]
    ));
    assert!(should_skip_tracked_file(
        Path::new("/tmp/tool-results/abc.txt"),
        None,
        None,
        &[]
    ));
}

#[test]
fn test_is_cacheable_read() {
    // Full reads are cacheable
    assert!(is_cacheable_read("Read", false, false));
    assert!(is_cacheable_read("ReadManyFiles", false, false));

    // Partial reads are not cacheable
    assert!(!is_cacheable_read("Read", true, false));
    assert!(!is_cacheable_read("Read", false, true));
    assert!(!is_cacheable_read("Read", true, true));

    // Metadata-only tools are not cacheable
    assert!(!is_cacheable_read("Glob", false, false));
    assert!(!is_cacheable_read("Grep", false, false));
}

#[test]
fn test_categorize_read_kind() {
    // Full content reads
    assert_eq!(
        categorize_read_kind("Read", false, false),
        FileReadKind::FullContent
    );
    assert_eq!(
        categorize_read_kind("ReadManyFiles", false, false),
        FileReadKind::FullContent
    );

    // Partial reads
    assert_eq!(
        categorize_read_kind("Read", true, false),
        FileReadKind::PartialContent
    );
    assert_eq!(
        categorize_read_kind("Read", false, true),
        FileReadKind::PartialContent
    );

    // Metadata-only
    assert_eq!(
        categorize_read_kind("Glob", false, false),
        FileReadKind::MetadataOnly
    );
    assert_eq!(
        categorize_read_kind("Grep", false, false),
        FileReadKind::MetadataOnly
    );
}

#[test]
fn test_is_stronger_kind() {
    // FullContent is stronger than PartialContent and MetadataOnly
    assert!(is_stronger_kind(
        &FileReadKind::FullContent,
        &FileReadKind::PartialContent
    ));
    assert!(is_stronger_kind(
        &FileReadKind::FullContent,
        &FileReadKind::MetadataOnly
    ));

    // PartialContent is stronger than MetadataOnly
    assert!(is_stronger_kind(
        &FileReadKind::PartialContent,
        &FileReadKind::MetadataOnly
    ));

    // Not stronger when equal or weaker
    assert!(!is_stronger_kind(
        &FileReadKind::FullContent,
        &FileReadKind::FullContent
    ));
    assert!(!is_stronger_kind(
        &FileReadKind::PartialContent,
        &FileReadKind::FullContent
    ));
    assert!(!is_stronger_kind(
        &FileReadKind::MetadataOnly,
        &FileReadKind::FullContent
    ));
}

#[test]
fn test_mention_read_decision() {
    // Line range always forces re-read
    assert_eq!(
        resolve_mention_read_decision(None, Path::new("/file.txt"), true),
        MentionReadDecision::NeedsReadLineRange
    );

    // No tracker means needs read
    assert_eq!(
        resolve_mention_read_decision(None, Path::new("/file.txt"), false),
        MentionReadDecision::NeedsRead
    );
}
