use super::*;
use crate::theme::Theme;

#[test]
fn test_parse_hunk_header_basic() {
    let hdr = parse_hunk_header("@@ -10,5 +20,7 @@ fn foo()").expect("should parse");
    assert_eq!(hdr.old_start, 10);
    assert_eq!(hdr.new_start, 20);
    assert_eq!(hdr.label, "fn foo()");
}

#[test]
fn test_parse_hunk_header_no_label() {
    let hdr = parse_hunk_header("@@ -1,3 +1,4 @@").expect("should parse");
    assert_eq!(hdr.old_start, 1);
    assert_eq!(hdr.new_start, 1);
    assert_eq!(hdr.label, "");
}

#[test]
fn test_parse_hunk_header_no_count() {
    let hdr = parse_hunk_header("@@ -1 +1 @@").expect("should parse");
    assert_eq!(hdr.old_start, 1);
    assert_eq!(hdr.new_start, 1);
}

#[test]
fn test_parse_hunk_header_invalid() {
    assert!(parse_hunk_header("not a hunk").is_none());
    assert!(parse_hunk_header("@@ garbage @@").is_none());
}

#[test]
fn test_render_diff_lines_basic() {
    let theme = Theme::default();
    let diff = "\
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 context
-old line
+new line
+added";
    let lines = render_diff_lines(diff, &theme, 80);
    // File headers(2) + hunk(1) + context(1) + paired old/new(2) + added(1) = 7
    assert_eq!(lines.len(), 7);
}

#[test]
fn test_render_diff_lines_empty() {
    let theme = Theme::default();
    let lines = render_diff_lines("", &theme, 80);
    assert!(lines.is_empty());
}

#[test]
fn test_render_structured_diff_scroll_past_end() {
    let theme = Theme::default();
    let diff = "+one\n+two";
    let lines = render_structured_diff("test.rs", diff, &theme, 80, 9999);
    assert!(lines.is_empty());
}

#[test]
fn test_render_structured_diff_negative_scroll() {
    let theme = Theme::default();
    let diff = "+one";
    let all = render_structured_diff("test.rs", diff, &theme, 80, 0);
    let neg = render_structured_diff("test.rs", diff, &theme, 80, -5);
    assert_eq!(all.len(), neg.len());
}

#[test]
fn test_truncate_path_short() {
    assert_eq!(truncate_path("foo.rs", 20), "foo.rs");
}

#[test]
fn test_truncate_path_long() {
    let long = "a/very/long/path/to/some/deeply/nested/file.rs";
    let result = truncate_path(long, 20);
    assert!(result.starts_with("..."));
    assert!(result.len() <= 20);
}

#[test]
fn test_truncate_path_tiny_max() {
    assert_eq!(truncate_path("abcdefgh", 3), "...");
}

#[test]
fn test_fmt_line_no_some() {
    assert_eq!(fmt_line_no(Some(42), 4), "  42");
}

#[test]
fn test_fmt_line_no_none() {
    assert_eq!(fmt_line_no(None, 4), "    ");
}

#[test]
fn test_classify_consecutive_removes_and_adds_paired() {
    let lines = vec!["-old1", "-old2", "+new1", "+new2"];
    let chunks = classify_diff_lines(&lines);
    let paired_count = chunks
        .iter()
        .filter(|c| matches!(c, DiffChunk::Paired { .. }))
        .count();
    assert_eq!(paired_count, 2);
}

#[test]
fn test_classify_unbalanced_removes_adds() {
    let lines = vec!["-old1", "-old2", "-old3", "+new1"];
    let chunks = classify_diff_lines(&lines);
    let paired = chunks
        .iter()
        .filter(|c| matches!(c, DiffChunk::Paired { .. }))
        .count();
    let removed = chunks
        .iter()
        .filter(|c| matches!(c, DiffChunk::Removed(_)))
        .count();
    assert_eq!(paired, 1);
    assert_eq!(removed, 2);
}
