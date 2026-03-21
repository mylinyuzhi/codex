use std::io::Write;

use tempfile::TempDir;

use super::*;

// ── truncate_content tests ──

#[test]
fn test_truncate_content_under_limit() {
    let content = "line 1\nline 2\nline 3";
    let (result, was_truncated) = truncate_content(content, 5);
    assert_eq!(result, content);
    assert!(!was_truncated);
}

#[test]
fn test_truncate_content_over_limit() {
    let content = "line 1\nline 2\nline 3\nline 4\nline 5";
    let (result, was_truncated) = truncate_content(content, 3);
    assert!(was_truncated);
    assert!(result.starts_with("line 1\nline 2\nline 3"));
    assert!(result.contains("truncated"));
    assert!(result.contains("5 total lines"));
}

#[test]
fn test_truncate_content_exact_limit() {
    let content = "line 1\nline 2\nline 3";
    let (result, was_truncated) = truncate_content(content, 3);
    assert_eq!(result, content);
    assert!(!was_truncated);
}

#[test]
fn test_truncate_content_zero_max_lines() {
    let content = "line 1\nline 2";
    let (result, was_truncated) = truncate_content(content, 0);
    assert_eq!(result, "");
    assert!(was_truncated);
}

#[test]
fn test_truncate_content_negative_max_lines() {
    let content = "line 1\nline 2";
    let (result, was_truncated) = truncate_content(content, -1);
    assert_eq!(result, "");
    assert!(was_truncated);
}

#[test]
fn test_truncate_content_empty_content() {
    let (result, was_truncated) = truncate_content("", 200);
    assert_eq!(result, "");
    assert!(!was_truncated);
}

#[test]
fn test_truncate_content_empty_content_zero_max() {
    let (result, was_truncated) = truncate_content("", 0);
    assert_eq!(result, "");
    assert!(!was_truncated); // empty content, nothing to truncate
}

#[test]
fn test_truncate_content_single_line() {
    let content = "only one line";
    let (result, was_truncated) = truncate_content(content, 1);
    assert_eq!(result, content);
    assert!(!was_truncated);
}

// ── frontmatter parsing tests ──

#[test]
fn test_parse_frontmatter_all_fields() {
    let content = "---\nname: test\ndescription: \"A test memory\"\ntype: user\n---\n# Content";
    let fm = parse_frontmatter(content, 20).unwrap();
    assert_eq!(fm.name, Some("test".to_string()));
    assert_eq!(fm.description, Some("A test memory".to_string()));
    assert_eq!(fm.memory_type, Some("user".to_string()));
}

#[test]
fn test_parse_frontmatter_description_only() {
    let content = "---\ndescription: Some description\n---\n# Content";
    let fm = parse_frontmatter(content, 20).unwrap();
    assert_eq!(fm.description, Some("Some description".to_string()));
    assert_eq!(fm.name, None);
    assert_eq!(fm.memory_type, None);
}

#[test]
fn test_parse_frontmatter_no_description() {
    let content = "---\nname: test\ntype: user\n---\n# Content";
    let fm = parse_frontmatter(content, 20).unwrap();
    assert_eq!(fm.name, Some("test".to_string()));
    assert_eq!(fm.description, None);
}

#[test]
fn test_parse_frontmatter_no_frontmatter() {
    let content = "# Just a markdown file\nNo frontmatter here.";
    assert!(parse_frontmatter(content, 20).is_none());
}

#[test]
fn test_parse_frontmatter_empty_values() {
    let content = "---\nname:\ndescription:\ntype:\n---\n# Content";
    // Empty values should not produce a frontmatter result
    assert!(parse_frontmatter(content, 20).is_none());
}

#[test]
fn test_parse_frontmatter_missing_closing_delimiter() {
    // Frontmatter without closing --- should NOT leak body content into metadata.
    // The body contains "description: leaky" which must not be picked up.
    let mut lines = vec!["---", "name: real", ""];
    // Push enough body lines to exceed the 20-line scan limit
    for i in 0..30 {
        lines.push(if i == 25 {
            "description: leaky"
        } else {
            "regular body content"
        });
    }
    let content = lines.join("\n");
    let fm = parse_frontmatter(&content, 20).unwrap();
    assert_eq!(fm.name.as_deref(), Some("real"));
    // "description: leaky" at line 28 must NOT have been picked up
    assert!(fm.description.is_none());
}

// ── frontmatter edge case tests ──

#[test]
fn test_parse_frontmatter_embedded_colon_in_value() {
    // Description contains a colon — should capture the full value after "description: "
    let content = "---\ndescription: API endpoint: /api/v2/users\n---\n";
    let fm = parse_frontmatter(content, 20).unwrap();
    assert_eq!(
        fm.description.as_deref(),
        Some("API endpoint: /api/v2/users"),
        "Embedded colons in unquoted values should be preserved"
    );
}

#[test]
fn test_parse_frontmatter_quoted_value_with_colon() {
    let content = "---\ndescription: \"key: value pair\"\n---\n";
    let fm = parse_frontmatter(content, 20).unwrap();
    assert_eq!(
        fm.description.as_deref(),
        Some("key: value pair"),
        "Outer quotes should be stripped, inner colon preserved"
    );
}

#[test]
fn test_parse_frontmatter_single_quoted_value() {
    let content = "---\nname: 'my memory'\n---\n";
    let fm = parse_frontmatter(content, 20).unwrap();
    assert_eq!(fm.name.as_deref(), Some("my memory"));
}

#[test]
fn test_parse_frontmatter_whitespace_around_value() {
    let content = "---\nname:   spaced   \ndescription:  trimmed  \n---\n";
    let fm = parse_frontmatter(content, 20).unwrap();
    assert_eq!(fm.name.as_deref(), Some("spaced"));
    assert_eq!(fm.description.as_deref(), Some("trimmed"));
}

#[test]
fn test_parse_frontmatter_max_lines_limit() {
    // With max_frontmatter_lines = 2, only lines 2-3 are scanned (after "---")
    let content = "---\nname: found\n\n\ndescription: missed\n---\n";
    let fm = parse_frontmatter(content, 2).unwrap();
    assert_eq!(fm.name.as_deref(), Some("found"));
    // description is on line 5 (4th line after ---), beyond max of 2
    assert!(fm.description.is_none());
}

// ── load/list tests ──

#[test]
fn test_load_memory_index_nonexistent() {
    let tmp = TempDir::new().unwrap();
    let result = load_memory_index(tmp.path(), 200).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_load_memory_index() {
    let tmp = TempDir::new().unwrap();
    let memory_path = tmp.path().join("MEMORY.md");
    let mut f = std::fs::File::create(&memory_path).unwrap();
    write!(f, "# Memory Index\n\n- [debug](debug.md) - Debugging notes").unwrap();

    let index = load_memory_index(tmp.path(), 200).unwrap().unwrap();
    assert!(!index.was_truncated);
    assert!(index.raw_content.contains("Memory Index"));
    assert_eq!(index.line_count, 3);
}

#[test]
fn test_list_memory_files_excludes_memory_md() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("MEMORY.md"), "index").unwrap();
    std::fs::write(tmp.path().join("debug.md"), "debug notes").unwrap();
    std::fs::write(tmp.path().join("patterns.md"), "patterns").unwrap();
    std::fs::write(tmp.path().join("notes.txt"), "not md").unwrap();

    let files = list_memory_files(tmp.path()).unwrap();
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|p| p.file_name().unwrap() != "MEMORY.md"));
    assert!(files.iter().all(|p| p.extension().unwrap() == "md"));
}

#[test]
fn test_load_memory_file_with_frontmatter() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.md");
    std::fs::write(
        &path,
        "---\nname: test\ndescription: \"A test file\"\ntype: feedback\n---\n# Content\nSome text.",
    )
    .unwrap();

    let entry = load_memory_file(&path, 200, 20).unwrap();
    assert_eq!(entry.description(), Some("A test file"));
    assert_eq!(entry.memory_type(), Some("feedback"));
    assert!(!entry.was_truncated);
}

// ── HTML comment stripping tests ──

#[test]
fn test_strip_html_comments_no_comments() {
    let content = "# Notes\nSome content here.";
    assert_eq!(strip_html_comments(content), content);
}

#[test]
fn test_strip_html_comments_single_comment() {
    let content = "before <!-- hidden --> after";
    assert_eq!(strip_html_comments(content), "before  after");
}

#[test]
fn test_strip_html_comments_multiline() {
    let content = "line 1\n<!-- multi\nline\ncomment -->\nline 2";
    assert_eq!(strip_html_comments(content), "line 1\n\nline 2");
}

#[test]
fn test_strip_html_comments_multiple() {
    let content = "a <!-- 1 --> b <!-- 2 --> c";
    assert_eq!(strip_html_comments(content), "a  b  c");
}

#[test]
fn test_strip_html_comments_unclosed() {
    let content = "before <!-- unclosed comment";
    assert_eq!(
        strip_html_comments(content),
        content,
        "Unclosed comment should be kept as-is"
    );
}

#[test]
fn test_strip_html_comments_empty_comment() {
    let content = "before <!----> after";
    assert_eq!(strip_html_comments(content), "before  after");
}

#[test]
fn test_load_memory_index_strips_comments() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("MEMORY.md");
    std::fs::write(&path, "# Index\n<!-- internal note -->\n- [a](a.md)").unwrap();

    let index = load_memory_index(tmp.path(), 200).unwrap().unwrap();
    assert!(!index.raw_content.contains("internal note"));
    assert!(index.raw_content.contains("# Index"));
    assert!(index.raw_content.contains("a.md"));
}

#[test]
fn test_load_memory_file_strips_comments() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("note.md");
    std::fs::write(
        &path,
        "---\nname: test\n---\n# Content\n<!-- private --> visible text",
    )
    .unwrap();

    let entry = load_memory_file(&path, 200, 20).unwrap();
    assert!(!entry.content.contains("private"));
    assert!(entry.content.contains("visible text"));
    // Frontmatter should still be parsed from raw content
    assert_eq!(
        entry.frontmatter.as_ref().unwrap().name.as_deref(),
        Some("test")
    );
}
