use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::theme::Theme;
use pretty_assertions::assert_eq;
use ratatui::style::Style;

use super::*;

/// Flatten a line's spans to plain text (drops styling).
fn line_text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn render(text: &str) -> Vec<Line<'static>> {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    render_markdown(
        text,
        MarkdownOptions::new(styles, 80, SyntaxHighlighting::Enabled),
        None,
    )
}

fn render_text(text: &str) -> Vec<String> {
    render(text).iter().map(line_text).collect()
}

#[test]
fn paragraph_is_indented_two_columns() {
    let lines = render_text("hello world");
    assert_eq!(lines, vec!["  hello world".to_string()]);
}

#[test]
fn soft_breaks_preserve_authored_lines() {
    let lines = render_text("first line\nsecond line");
    assert_eq!(
        lines,
        vec!["  first line".to_string(), "  second line".to_string()]
    );
}

#[test]
fn heading_renders_without_hash_prefix() {
    let lines = render_text("# Title");
    assert_eq!(lines, vec!["  Title".to_string()]);
}

#[test]
fn unordered_list_uses_bullet_marker() {
    let lines = render_text("- one\n- two");
    assert_eq!(lines, vec!["  • one".to_string(), "  • two".to_string()]);
}

#[test]
fn ordered_list_numbers_items() {
    let lines = render_text("1. first\n2. second");
    assert_eq!(
        lines,
        vec!["  1. first".to_string(), "  2. second".to_string()]
    );
}

#[test]
fn fenced_code_block_has_border_and_body() {
    let lines = render_text("```rust\nlet x = 1;\n```");
    assert!(
        lines
            .first()
            .is_some_and(|l| l.trim_start().starts_with('┌'))
    );
    assert!(lines.iter().any(|l| l.contains("let x = 1;")));
    assert!(
        lines
            .last()
            .is_some_and(|l| l.trim_start().starts_with('└'))
    );
}

#[test]
fn table_renders_box_grid() {
    let md = "| a | b |\n| - | - |\n| 1 | 2 |";
    let lines = render_text(md);
    assert!(lines.iter().any(|l| l.trim_start().starts_with('┌')));
    assert!(lines.iter().any(|l| l.contains('a') && l.contains('b')));
    assert!(lines.iter().any(|l| l.contains('1') && l.contains('2')));
    assert!(lines.iter().any(|l| l.trim_start().starts_with('└')));
}

#[test]
fn gfm_alert_emits_labeled_header() {
    let lines = render_text("> [!WARNING]\n> be careful");
    assert!(lines.iter().any(|l| l.contains("WARNING")));
    assert!(lines.iter().any(|l| l.contains("be careful")));
}

#[test]
fn lead_marker_lands_on_first_line() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let marker = LeadMarker::new("⏺", styles.assistant_message());
    let opts = MarkdownOptions::new(styles, 80, SyntaxHighlighting::Enabled);
    let lines = render_markdown("hello\nworld", opts, Some(&marker));
    assert_eq!(line_text(&lines[0]), "⏺ hello");
    assert_eq!(line_text(&lines[1]), "  world");
}

#[test]
fn empty_text_with_marker_emits_marker_line() {
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let marker = LeadMarker::new("⏺", styles.assistant_message());
    let opts = MarkdownOptions::new(styles, 80, SyntaxHighlighting::Enabled);
    let lines = render_markdown("", opts, Some(&marker));
    assert_eq!(lines.len(), 1);
    assert_eq!(line_text(&lines[0]), "⏺");
}

#[test]
fn empty_text_without_marker_is_empty() {
    let _ = Style::default();
    assert!(render("").is_empty());
}

#[test]
fn list_item_text_survives_following_block() {
    // Regression: a list item's text immediately followed by a block (code
    // fence) must not be dropped (was overwritten by the raw block line).
    let joined = render_text("- step one\n  ```\n  code\n  ```\n").join("\n");
    assert!(joined.contains("step one"), "item text dropped:\n{joined}");
    assert!(joined.contains("code"), "code body dropped:\n{joined}");
}

#[test]
fn nested_list_preserves_parent_text_and_marker() {
    // Regression: parent item text + marker must render on their own line
    // before descending, not merge with the first child.
    let lines = render_text("- outer\n  - inner1\n  - inner2\n");
    assert!(
        lines.iter().any(|l| l.trim() == "• outer"),
        "parent merged/lost:\n{lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains("inner1")));
    assert!(lines.iter().any(|l| l.contains("inner2")));
}

#[test]
fn task_list_uses_checkbox_not_redundant_bullet() {
    let lines = render_text("- [ ] todo\n- [x] done\n");
    assert_eq!(lines, vec!["  ☐ todo".to_string(), "  ☑ done".to_string()]);
}

#[test]
fn list_continuation_hangs_under_item_text() {
    let lines = render_text("- line1\n  line2\n");
    assert_eq!(
        lines,
        vec!["  • line1".to_string(), "    line2".to_string()]
    );
}

#[test]
fn multiline_html_block_keeps_line_structure() {
    let joined = render_text("before\n\n<div>\nraw html\n</div>\n\nafter").join("\n");
    assert!(joined.contains("<div>"), "{joined}");
    assert!(joined.contains("raw html"), "{joined}");
    assert!(joined.contains("</div>"), "{joined}");
}

#[cfg(feature = "mermaid")]
#[test]
fn mermaid_fence_renders_as_cells() {
    let lines = render("```mermaid\nflowchart LR\n  A[Start] --> B[Finish]\n```");
    let joined: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(
        joined.contains("Start") && joined.contains("Finish"),
        "node labels missing:\n{joined}"
    );
    assert!(
        joined
            .chars()
            .any(|c| matches!(c, '╭' | '╮' | '╰' | '╯' | '│' | '─')),
        "expected box-drawing:\n{joined}"
    );
    // Rendered as a diagram, not the verbatim `flowchart LR` source line.
    assert!(
        !joined.contains("flowchart LR"),
        "fell back to verbatim:\n{joined}"
    );
}

#[test]
fn table_with_wide_cell_keeps_uniform_row_width() {
    // F3: a CJK cell wider than the column cap must not leave its row a column
    // short — pad_cell re-pads to the exact column width after truncation.
    let md = "| a | b |\n| - | - |\n| 你好世界你好世界 | y |";
    let widths: Vec<usize> = render_text(md)
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| coco_tui_ui::truncate::display_width(l))
        .collect();
    assert!(widths.len() >= 4, "expected a full table grid: {widths:?}");
    assert!(
        widths.windows(2).all(|w| w[0] == w[1]),
        "table rows have differing display widths: {widths:?}"
    );
}

#[test]
fn sibling_ordered_lists_restart_numbering() {
    let lines = render_text("1. a\n2. b\n\ntext\n\n1. c\n2. d");
    assert!(lines.iter().any(|l| l.contains("1. a")));
    assert!(
        lines.iter().any(|l| l.contains("1. c")),
        "second ordered list did not restart at 1: {lines:?}"
    );
}

#[test]
fn rule_in_blockquote_fits_width() {
    // F4: the rule budget subtracts the full left margin (indent + quote gutter),
    // so it does not overflow the 80-col width inside a blockquote.
    let lines = render_text("> intro\n>\n> ---\n");
    let rule = lines
        .iter()
        .find(|l| l.contains('─'))
        .expect("rule rendered");
    assert!(
        coco_tui_ui::truncate::display_width(rule) <= 80,
        "blockquote rule overflows width: {} ({rule:?})",
        coco_tui_ui::truncate::display_width(rule)
    );
}

#[test]
fn code_fence_in_blockquote_fits_width() {
    // F4: fence border + bg-padded body budget from the full left margin.
    let lines = render_text("> ```\n> code here\n> ```\n");
    for l in lines.iter().filter(|l| !l.trim().is_empty()) {
        assert!(
            coco_tui_ui::truncate::display_width(l) <= 80,
            "blockquote code-fence line overflows width: {} ({l:?})",
            coco_tui_ui::truncate::display_width(l)
        );
    }
}

#[cfg(feature = "mermaid")]
#[test]
fn streaming_suppresses_mermaid_diagram() {
    // The same diagram-capable fence must NOT be laid out while streaming —
    // it stays verbatim so the block doesn't reflow on every delta.
    let theme = Theme::default();
    let styles = UiStyles::new(&theme);
    let src = "```mermaid\nflowchart LR\n  A[Start] --> B[Finish]\n```";
    let lines = render_markdown(
        src,
        MarkdownOptions::new(styles, 80, SyntaxHighlighting::Enabled).streaming(),
        None,
    );
    let joined: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(
        joined.contains("flowchart LR"),
        "streaming should keep the verbatim source, got:\n{joined}"
    );
}
