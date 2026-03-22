use super::*;
use crate::theme::Theme;

#[test]
fn test_plain_text() {
    let theme = Theme::default();
    let lines = markdown_to_lines("Hello world", &theme, 80);
    assert!(!lines.is_empty());
}

#[test]
fn test_header() {
    let theme = Theme::default();
    let lines = markdown_to_lines("# Title", &theme, 80);
    assert!(!lines.is_empty());
}

#[test]
fn test_code_block() {
    let theme = Theme::default();
    let text = "```rust\nfn main() {}\n```";
    let lines = markdown_to_lines(text, &theme, 80);
    assert!(lines.len() >= 3);
}

#[test]
fn test_list_items() {
    let theme = Theme::default();
    let text = "- item 1\n- item 2";
    let lines = markdown_to_lines(text, &theme, 80);
    assert_eq!(lines.len(), 2);
}
