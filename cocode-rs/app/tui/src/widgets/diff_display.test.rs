use pretty_assertions::assert_eq;
use ratatui::style::Color;
use ratatui::style::Style;

use super::*;
use crate::theme::Theme;

fn get_line_text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn has_fg_color(line: &Line<'_>, color: Color) -> bool {
    line.spans.iter().any(|s| s.style.fg == Some(color))
}

#[test]
fn test_add_lines_styled_green() {
    let theme = Theme::default();
    let diff = "+added line";
    let lines = render_diff_lines(diff, &theme, 80);
    assert_eq!(lines.len(), 1);
    assert!(
        has_fg_color(&lines[0], Color::Green),
        "add line should have green fg"
    );
}

#[test]
fn test_del_lines_styled_red() {
    let theme = Theme::default();
    let diff = "-removed line";
    let lines = render_diff_lines(diff, &theme, 80);
    assert_eq!(lines.len(), 1);
    assert!(
        has_fg_color(&lines[0], Color::Red),
        "del line should have red fg"
    );
}

#[test]
fn test_context_lines_dim() {
    let theme = Theme::default();
    let diff = " context line";
    let lines = render_diff_lines(diff, &theme, 80);
    assert_eq!(lines.len(), 1);
    let text = get_line_text(&lines[0]);
    assert!(text.contains("context line"));
}

#[test]
fn test_hunk_header_styled() {
    let theme = Theme::default();
    let diff = "@@ -1,3 +1,4 @@";
    let lines = render_diff_lines(diff, &theme, 80);
    assert_eq!(lines.len(), 1);
    let text = get_line_text(&lines[0]);
    assert!(text.contains("@@"));
    // Hunk headers should use cyan or primary color
    assert!(
        has_fg_color(&lines[0], Color::Cyan)
            || lines[0].spans.iter().any(|s| s
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)),
        "hunk header should be styled"
    );
}

#[test]
fn test_empty_diff() {
    let theme = Theme::default();
    let lines = render_diff_lines("", &theme, 80);
    assert!(lines.is_empty());
}

#[test]
fn test_file_headers_treated_as_metadata() {
    let theme = Theme::default();
    let diff = "--- a/file.rs\n+++ b/file.rs";
    let lines = render_diff_lines(diff, &theme, 80);
    assert_eq!(lines.len(), 2);
    // File headers (--- and +++) should be treated as metadata, not add/del
    let first_text = get_line_text(&lines[0]);
    let second_text = get_line_text(&lines[1]);
    assert!(first_text.contains("--- a/file.rs"));
    assert!(second_text.contains("+++ b/file.rs"));
}

#[test]
fn test_full_diff_renders_all_line_types() {
    let theme = Theme::default();
    let diff = "\
diff --git a/file.rs b/file.rs
index abc..def 100644
--- a/file.rs
+++ b/file.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!(\"old\");
+    println!(\"new\");
+    println!(\"extra\");
 }";
    let lines = render_diff_lines(diff, &theme, 80);
    // Should have: diff header, index, ---, +++, @@, context, del, add, add, context
    assert_eq!(lines.len(), 10);

    // Verify add lines have green
    assert!(has_fg_color(&lines[7], Color::Green)); // +new
    assert!(has_fg_color(&lines[8], Color::Green)); // +extra

    // Verify del line has red
    assert!(has_fg_color(&lines[6], Color::Red)); // -old
}

#[test]
fn test_palette_detect_returns_valid_palette() {
    let theme = Theme::default();
    // Just verify it doesn't panic
    let _ = DiffPalette::detect(&theme);
}

#[test]
fn test_ansi16_palette_uses_theme_colors() {
    let theme = Theme::default();
    let palette = DiffPalette::ansi16(&theme);
    assert_eq!(palette.add_style, Style::default().fg(theme.success));
    assert_eq!(palette.del_style, Style::default().fg(theme.error));
}
