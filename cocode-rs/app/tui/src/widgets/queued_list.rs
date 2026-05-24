//! Queued commands list widget.
//!
//! Displays queued commands waiting to be processed. These commands
//! were entered during streaming and will be consumed once as steering
//! system-reminders that ask the model to address each message.

use cocode_protocol::UserQueuedCommand;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::theme::Theme;

/// Widget to render queued commands above the input box.
///
/// Layout (aligns with Claude Code's QueuedCommandsList):
/// ```text
/// 🕐 Waiting:
///   • "use TypeScript instead"
///   • "add error handling"
/// ```
pub struct QueuedListWidget<'a> {
    commands: &'a [UserQueuedCommand],
    theme: &'a Theme,
    max_display: i32,
}

impl<'a> QueuedListWidget<'a> {
    /// Create a new queued list widget.
    pub fn new(commands: &'a [UserQueuedCommand], theme: &'a Theme) -> Self {
        Self {
            commands,
            theme,
            max_display: 5,
        }
    }

    /// Set the maximum number of commands to display.
    pub fn max_display(mut self, max: i32) -> Self {
        self.max_display = max;
        self
    }

    /// Calculate the height needed to render the queued list.
    ///
    /// Returns 0 if there are no queued commands.
    pub fn required_height(&self) -> u16 {
        if self.commands.is_empty() {
            return 0;
        }

        let count = (self.commands.len() as i32).min(self.max_display) as u16;
        // 1 line for header + 1 line per command
        1 + count
    }
}

impl Widget for QueuedListWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.commands.is_empty() || area.height < 2 || area.width < 10 {
            return;
        }

        let mut lines: Vec<Line> = Vec::new();

        // Header line: "🕐 Waiting:" (dimmed)
        let waiting_label = t!("status.waiting").to_string();
        lines.push(Line::from(
            Span::raw(format!("  {waiting_label}")).fg(self.theme.text_dim),
        ));

        // Command lines
        for cmd in self.commands.iter().take(self.max_display as usize) {
            // Truncate the prompt if too long
            let max_len = area.width.saturating_sub(8) as usize; // "    • " prefix + quotes
            let prompt = if cmd.prompt.len() > max_len {
                format!("{}...", &cmd.prompt[..max_len.saturating_sub(3)])
            } else {
                cmd.prompt.clone()
            };

            lines.push(Line::from(vec![
                Span::raw("    ").fg(self.theme.text_dim),
                Span::raw("• ").fg(self.theme.text_dim),
                Span::raw(format!("\"{prompt}\"")).fg(self.theme.text_dim),
            ]));
        }

        // Show "+N more" if there are more commands than displayed
        let remaining = self.commands.len() as i32 - self.max_display;
        if remaining > 0 {
            lines.push(Line::from(
                Span::raw(format!("    +{remaining} more..."))
                    .fg(self.theme.text_dim)
                    .italic(),
            ));
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
#[path = "queued_list.test.rs"]
mod tests;
