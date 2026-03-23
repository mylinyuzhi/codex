use super::*;

#[test]
fn test_new_display_starts_at_zero() {
    let display = StreamDisplay::new();
    assert_eq!(display.cursor(), 0);
}

#[test]
fn test_advance_one_line() {
    let mut display = StreamDisplay::new();
    display.on_content_appended(12);
    let content = "hello\nworld\n";
    assert!(display.advance(content));
    assert_eq!(display.cursor(), 6); // "hello\n"
    assert!(display.advance(content));
    assert_eq!(display.cursor(), 12); // "hello\nworld\n"
}

#[test]
fn test_advance_no_newline_does_not_advance() {
    let mut display = StreamDisplay::new();
    display.on_content_appended(5);
    let content = "hello";
    // No newline → stays at 0 in smooth mode
    assert!(!display.advance(content));
    assert_eq!(display.cursor(), 0);
}

#[test]
fn test_reveal_all() {
    let mut display = StreamDisplay::new();
    display.on_content_appended(12);
    display.reveal_all(12);
    assert_eq!(display.cursor(), 12);
}

#[test]
fn test_reset() {
    let mut display = StreamDisplay::new();
    display.on_content_appended(12);
    display.reveal_all(12);
    display.reset();
    assert_eq!(display.cursor(), 0);
}

#[test]
fn test_no_advance_when_caught_up() {
    let mut display = StreamDisplay::new();
    let content = "done\n";
    display.on_content_appended(content.len());
    assert!(display.advance(content));
    assert_eq!(display.cursor(), 5);
    // Already caught up
    assert!(!display.advance(content));
}

#[test]
fn test_pending_since_cleared_when_caught_up() {
    let mut display = StreamDisplay::new();
    let content = "line\n";
    display.on_content_appended(content.len());
    assert!(display.pending_since.is_some());
    assert!(display.advance(content));
    assert!(display.pending_since.is_none());
}

#[test]
fn test_count_newlines() {
    assert_eq!(count_newlines(""), 0);
    assert_eq!(count_newlines("hello"), 0);
    assert_eq!(count_newlines("hello\n"), 1);
    assert_eq!(count_newlines("a\nb\nc\n"), 3);
    assert_eq!(count_newlines("\n\n\n"), 3);
}
