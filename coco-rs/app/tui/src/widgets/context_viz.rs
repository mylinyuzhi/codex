//! Context window visualization widget.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// Context window visualization showing token usage as a horizontal bar.
pub struct ContextVizWidget<'a> {
    used: i32,
    total: i32,
    input: i64,
    output: i64,
    cache_read: i64,
    theme: &'a Theme,
}

impl<'a> ContextVizWidget<'a> {
    pub fn new(
        used: i32,
        total: i32,
        input: i64,
        output: i64,
        cache_read: i64,
        theme: &'a Theme,
    ) -> Self {
        Self {
            used,
            total,
            input,
            output,
            cache_read,
            theme,
        }
    }
}

impl Widget for ContextVizWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height == 0 {
            return;
        }

        let pct = if self.total > 0 {
            ((self.used as f64 / self.total as f64) * 100.0) as i32
        } else {
            0
        };
        let progress = if self.total > 0 {
            (self.used as f64 / self.total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let mut lines: Vec<Line> = Vec::new();

        // Line 1: progress bar
        lines.push(build_bar_line(
            area.width.saturating_sub(4), // account for block borders + padding
            progress,
            pct,
            self.theme,
        ));

        // Line 2: breakdown
        let breakdown = format!(
            "Input: {} | Output: {} | Cache: {}",
            format_number(self.input),
            format_number(self.output),
            format_number(self.cache_read),
        );
        lines.push(Line::from(Span::raw(breakdown).dim()));

        // Line 3: total
        let total_line = format!(
            "Used: {} / {}",
            format_number(self.used as i64),
            format_number(self.total as i64),
        );
        lines.push(Line::from(Span::raw(total_line).dim()));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Context Window ")
            .border_style(Style::default().fg(self.theme.border));

        Paragraph::new(lines).block(block).render(area, buf);
    }
}

/// Build the bar line: `[████████░░░░░░░░░░░░] 42%`
fn build_bar_line<'a>(available_width: u16, progress: f64, pct: i32, theme: &Theme) -> Line<'a> {
    let pct_label = format!(" {pct}%");
    // brackets + pct label
    let overhead = 2 + pct_label.len() as u16;
    let bar_width = (available_width as i32 - overhead as i32).max(0) as u16;

    let filled = ((bar_width as f64) * progress) as u16;
    let empty = bar_width.saturating_sub(filled);

    let filled_str: String = std::iter::repeat_n('\u{2588}', filled as usize).collect();
    let empty_str: String = std::iter::repeat_n('\u{2591}', empty as usize).collect();

    Line::from(vec![
        Span::raw("[").fg(theme.text_dim),
        Span::raw(filled_str).fg(theme.context_used),
        Span::raw(empty_str).fg(theme.context_free),
        Span::raw("]").fg(theme.text_dim),
        Span::raw(pct_label).bold().fg(theme.text),
    ])
}

/// Format a number with comma separators.
fn format_number(n: i64) -> String {
    if n < 0 {
        return format!("-{}", format_number(-n));
    }
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}
