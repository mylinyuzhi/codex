//! Toast notification widget — auto-expiring messages at top-right.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::state::ui::Toast;
use crate::state::ui::ToastSeverity;
use crate::theme::Theme;

/// Toast notification display.
pub struct ToastWidget<'a> {
    toasts: &'a [Toast],
    theme: &'a Theme,
}

impl<'a> ToastWidget<'a> {
    pub fn new(toasts: &'a [Toast], theme: &'a Theme) -> Self {
        Self { toasts, theme }
    }
}

impl Widget for ToastWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let toast_width: u16 = 40;
        let mut y = 1_u16;

        for toast in self.toasts {
            if y >= area.height.saturating_sub(2) {
                break;
            }

            let (icon, color) = match toast.severity {
                ToastSeverity::Info => ("ℹ", self.theme.text_dim),
                ToastSeverity::Success => ("✓", self.theme.success),
                ToastSeverity::Warning => ("⚠", self.theme.warning),
                ToastSeverity::Error => ("✗", self.theme.error),
            };

            let x = area.width.saturating_sub(toast_width + 1);
            let toast_area = Rect::new(area.x + x, area.y + y, toast_width, 1);

            // Truncate message to fit
            let max_msg_len = (toast_width as usize).saturating_sub(5);
            let msg = if toast.message.len() > max_msg_len {
                format!("{}...", &toast.message[..max_msg_len.saturating_sub(3)])
            } else {
                toast.message.clone()
            };

            let text = format!(" {icon} {msg} ");
            Clear.render(toast_area, buf);
            Paragraph::new(Span::raw(text).fg(color)).render(toast_area, buf);

            y += 1;
        }
    }
}
