use super::*;
use cocode_protocol::ToolName;

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
fn test_is_full_content_read_tool() {
    assert!(is_full_content_read_tool(ToolName::Read.as_str()));
    assert!(is_full_content_read_tool("ReadManyFiles"));

    assert!(!is_full_content_read_tool(ToolName::Glob.as_str()));
    assert!(!is_full_content_read_tool(ToolName::Grep.as_str()));
    assert!(!is_full_content_read_tool(ToolName::Edit.as_str()));
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
    assert!(is_cacheable_read(ToolName::Read.as_str(), false, false));
    assert!(is_cacheable_read("ReadManyFiles", false, false));

    // Partial reads are not cacheable
    assert!(!is_cacheable_read(ToolName::Read.as_str(), true, false));
    assert!(!is_cacheable_read(ToolName::Read.as_str(), false, true));
    assert!(!is_cacheable_read(ToolName::Read.as_str(), true, true));

    // Metadata-only tools are not cacheable
    assert!(!is_cacheable_read(ToolName::Glob.as_str(), false, false));
    assert!(!is_cacheable_read(ToolName::Grep.as_str(), false, false));
}

#[test]
fn test_categorize_read_kind() {
    // Full content reads
    assert_eq!(
        categorize_read_kind(ToolName::Read.as_str(), false, false),
        FileReadKind::FullContent
    );
    assert_eq!(
        categorize_read_kind("ReadManyFiles", false, false),
        FileReadKind::FullContent
    );

    // Partial reads
    assert_eq!(
        categorize_read_kind(ToolName::Read.as_str(), true, false),
        FileReadKind::PartialContent
    );
    assert_eq!(
        categorize_read_kind(ToolName::Read.as_str(), false, true),
        FileReadKind::PartialContent
    );

    // Metadata-only
    assert_eq!(
        categorize_read_kind(ToolName::Glob.as_str(), false, false),
        FileReadKind::MetadataOnly
    );
    assert_eq!(
        categorize_read_kind(ToolName::Grep.as_str(), false, false),
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
