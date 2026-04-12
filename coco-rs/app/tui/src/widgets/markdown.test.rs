//! Tests for markdown rendering.

use crate::theme::Theme;
use crate::widgets::markdown::markdown_to_lines;

#[test]
fn test_plain_text() {
    let theme = Theme::default();
    let lines = markdown_to_lines("Hello world", &theme, 80);
    assert_eq!(lines.len(), 1);
}

#[test]
fn test_headers() {
    let theme = Theme::default();
    let text = "# H1\n## H2\n### H3";
    let lines = markdown_to_lines(text, &theme, 80);
    assert_eq!(lines.len(), 3);
}

#[test]
fn test_code_block() {
    let theme = Theme::default();
    let text = "```rust\nfn main() {}\n```";
    let lines = markdown_to_lines(text, &theme, 80);
    // fence open + code line + fence close
    assert_eq!(lines.len(), 3);
}

#[test]
fn test_list_items() {
    let theme = Theme::default();
    let text = "- item 1\n- item 2\n* item 3";
    let lines = markdown_to_lines(text, &theme, 80);
    assert_eq!(lines.len(), 3);
}

#[test]
fn test_blockquote() {
    let theme = Theme::default();
    let text = "> quoted text\n> more";
    let lines = markdown_to_lines(text, &theme, 80);
    assert_eq!(lines.len(), 2);
}

#[test]
fn test_empty_lines_preserved() {
    let theme = Theme::default();
    let text = "para 1\n\npara 2";
    let lines = markdown_to_lines(text, &theme, 80);
    assert_eq!(lines.len(), 3); // para1 + empty + para2
}

#[test]
fn test_numbered_list() {
    let theme = Theme::default();
    let text = "1. first\n2. second";
    let lines = markdown_to_lines(text, &theme, 80);
    assert_eq!(lines.len(), 2);
}
