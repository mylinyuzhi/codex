//! History-line preparation for native scrollback insertion.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

/// Render finalized history lines into a buffer at the target width.
///
/// Lines are **wrapped** to `width` (word-wrap, `trim: false`) so a committed
/// scrollback line occupies the exact same rows it did in the live tail — which
/// paints with `Paragraph::wrap(Wrap { trim: false })`. Without wrapping here, a
/// logical line longer than `width` was clipped to a single row on commit,
/// silently dropping its wrapped continuation and desyncing native scrollback
/// from the live render. The buffer height is the wrapped row count
/// (`Paragraph::line_count`, the same `WordWrapper` the renderer uses), not
/// `lines.len()`, so the caller inserts the correct number of scrollback rows.
pub fn render_history_lines(lines: Vec<Line<'static>>, width: u16) -> Buffer {
    if width == 0 || lines.is_empty() {
        return Buffer::empty(Rect::new(0, 0, width, 0));
    }
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let height = paragraph.line_count(width).min(u16::MAX as usize) as u16;
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    if height > 0 {
        paragraph.render(area, &mut buffer);
    }
    buffer
}

#[cfg(test)]
#[path = "history_insert.test.rs"]
mod tests;
