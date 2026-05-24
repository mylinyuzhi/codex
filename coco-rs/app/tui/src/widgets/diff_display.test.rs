use super::*;
use crate::presentation::styles::UiStyles;
use crate::theme::Theme;

#[test]
fn test_render_diff_lines_basic() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let diff = "\
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 context
-old line
+new line
+added";
    let lines = render_diff_lines(diff, styles, 80);
    // File headers(2) + hunk(1) + context(1) + paired old/new(2) + added(1) = 7
    assert_eq!(lines.len(), 7);
}

#[test]
fn test_render_diff_lines_empty() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let lines = render_diff_lines("", styles, 80);
    assert!(lines.is_empty());
}

#[test]
fn test_render_structured_diff_scroll_past_end() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let diff = "+one\n+two";
    let lines = render_structured_diff("test.rs", diff, styles, 80, 9999);
    assert!(lines.is_empty());
}

#[test]
fn test_render_structured_diff_negative_scroll() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let diff = "+one";
    let all = render_structured_diff("test.rs", diff, styles, 80, 0);
    let neg = render_structured_diff("test.rs", diff, styles, 80, -5);
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
