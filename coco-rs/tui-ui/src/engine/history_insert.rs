//! History-line preparation for native scrollback insertion.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

/// Render finalized history lines into a buffer at the target width.
pub fn render_history_lines(lines: Vec<Line<'static>>, width: u16) -> Buffer {
    let height = lines.len() as u16;
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    if width > 0 && height > 0 {
        Paragraph::new(lines).render(area, &mut buffer);
    }
    buffer
}

#[cfg(test)]
#[path = "history_insert.test.rs"]
mod tests;
