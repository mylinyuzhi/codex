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
