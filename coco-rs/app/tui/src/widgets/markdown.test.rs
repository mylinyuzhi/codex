//! Tests for markdown rendering.

use crate::display_settings::SyntaxHighlighting;
use crate::theme::Theme;
use crate::widgets::markdown::markdown_to_lines;
use crate::widgets::markdown::markdown_to_lines_with_syntax;

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
fn test_code_block_syntax_highlighting_can_be_disabled() {
    let theme = Theme::default();
    let text = "```rust\nfn main() {}\n```";

    let highlighted = markdown_to_lines_with_syntax(text, &theme, 80, SyntaxHighlighting::Enabled);
    assert!(
        highlighted[1]
            .spans
            .iter()
            .any(|span| span.style.fg == Some(theme.code_keyword))
    );

    let plain = markdown_to_lines_with_syntax(text, &theme, 80, SyntaxHighlighting::Disabled);
    assert!(
        !plain[1]
            .spans
            .iter()
            .any(|span| span.style.fg == Some(theme.code_keyword))
    );
    assert_eq!(plain[1].spans[1].content.as_ref(), "fn main() {}");
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
