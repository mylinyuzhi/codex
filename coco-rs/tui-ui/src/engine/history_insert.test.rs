use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::text::Line;

use super::*;

#[test]
fn render_history_lines_preserves_width_and_row_count() {
    let buffer = render_history_lines(vec![Line::from("one"), Line::from("two")], 6);

    assert_eq!(buffer.area.width, 6);
    assert_eq!(buffer.area.height, 2);
    assert_eq!(buffer, Buffer::with_lines(["one   ", "two   "]));
}

#[test]
fn render_history_lines_handles_empty_input() {
    let buffer = render_history_lines(Vec::new(), 6);

    assert_eq!(buffer.area.width, 6);
    assert_eq!(buffer.area.height, 0);
}

#[test]
fn render_history_lines_wraps_long_line_instead_of_clipping() {
    // A logical line longer than `width` must occupy multiple rows (matching the
    // live tail's Paragraph::wrap), NOT be clipped to a single row — otherwise
    // committed scrollback silently drops the wrapped continuation.
    let buffer = render_history_lines(vec![Line::from("alpha bravo cobra eagle")], 6);

    assert_eq!(buffer.area.width, 6);
    assert!(
        buffer.area.height >= 2,
        "expected wrapped rows, got height {}",
        buffer.area.height
    );
    let text: String = (0..buffer.area.height)
        .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
        .map(|(x, y)| buffer[(x, y)].symbol().to_string())
        .collect();
    assert!(
        text.contains("bravo"),
        "wrapped continuation lost: {text:?}"
    );
    assert!(
        text.contains("eagle"),
        "wrapped continuation lost: {text:?}"
    );
}
