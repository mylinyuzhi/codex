//! Reusable progress bar widget.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// A single-line progress bar: `[████████░░░░░░░░░░░░] 42%`
pub struct ProgressBarWidget<'a> {
    progress: f64,
    theme: &'a Theme,
    label: Option<&'a str>,
    filled_char: char,
    empty_char: char,
}

impl<'a> ProgressBarWidget<'a> {
    pub fn new(progress: f64, theme: &'a Theme) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            theme,
            label: None,
            filled_char: '\u{2588}', // █
            empty_char: '\u{2591}',  // ░
        }
    }

    /// Set an optional label displayed after the percentage.
    pub fn label(mut self, text: &'a str) -> Self {
        self.label = Some(text);
        self
    }

    /// Override the character used for the filled portion.
    pub fn filled_char(mut self, ch: char) -> Self {
        self.filled_char = ch;
        self
    }

    /// Override the character used for the empty portion.
    pub fn empty_char(mut self, ch: char) -> Self {
        self.empty_char = ch;
        self
    }
}

impl Widget for ProgressBarWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let pct = (self.progress * 100.0) as i32;
        let pct_text = format!(" {pct}%");
        let label_text = self.label.map(|l| format!(" {l}")).unwrap_or_default();

        // Reserve space for brackets, percentage, and optional label.
        // Layout: `[<bar>] XX%<label>`
        let overhead = 2 + pct_text.len() as u16 + label_text.len() as u16;
        let bar_width = (area.width as i32 - overhead as i32).max(0) as u16;

        let filled = ((bar_width as f64) * self.progress) as u16;
        let empty = bar_width.saturating_sub(filled);

        let bar: String = std::iter::repeat_n(self.filled_char, filled as usize)
            .chain(std::iter::repeat_n(self.empty_char, empty as usize))
            .collect();

        let full_text = format!("[{bar}]{pct_text}{label_text}");

        let y = area.y;
        let mut x = area.x;

        // Render character by character with coloring
        for ch in full_text.chars() {
            if x >= area.x + area.width {
                break;
            }
            let style = if ch == self.filled_char {
                Style::default().fg(self.theme.progress_bar)
            } else if ch == self.empty_char {
                Style::default().fg(self.theme.context_free)
            } else {
                Style::default().fg(self.theme.text_dim)
            };
            buf.set_string(x, y, ch.to_string(), style);
            x += 1;
        }
    }
}
