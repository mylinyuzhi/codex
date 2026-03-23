//! Header bar widget.
//!
//! Displays session context at the top of the screen:
//! - Left: session name, working directory, turn count
//! - Right: compaction/fallback indicators

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::theme::Theme;

/// Header bar widget showing session context.
pub struct HeaderBar<'a> {
    theme: &'a Theme,
    session_id: Option<&'a str>,
    working_dir: Option<&'a str>,
    turn_count: i32,
    is_compacting: bool,
    fallback_model: Option<&'a str>,
    active_worktrees: i32,
}

impl<'a> HeaderBar<'a> {
    /// Create a new header bar.
    pub fn new(theme: &'a Theme) -> Self {
        Self {
            theme,
            session_id: None,
            working_dir: None,
            turn_count: 0,
            is_compacting: false,
            fallback_model: None,
            active_worktrees: 0,
        }
    }

    /// Set the session ID.
    pub fn session_id(mut self, id: Option<&'a str>) -> Self {
        self.session_id = id;
        self
    }

    /// Set the working directory.
    pub fn working_dir(mut self, dir: Option<&'a str>) -> Self {
        self.working_dir = dir;
        self
    }

    /// Set the turn count.
    pub fn turn_count(mut self, count: i32) -> Self {
        self.turn_count = count;
        self
    }

    /// Set whether compaction is in progress.
    pub fn is_compacting(mut self, compacting: bool) -> Self {
        self.is_compacting = compacting;
        self
    }

    /// Set the fallback model name.
    pub fn fallback_model(mut self, model: Option<&'a str>) -> Self {
        self.fallback_model = model;
        self
    }

    /// Set the active worktree count.
    pub fn active_worktrees(mut self, count: i32) -> Self {
        self.active_worktrees = count;
        self
    }

    /// Maximum display length for working directory in the header.
    const HEADER_PATH_MAX_LEN: i32 = 40;
}

impl Widget for HeaderBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 1 || area.width < 10 {
            return;
        }

        let mut left_spans: Vec<Span> = Vec::new();
        let mut right_spans: Vec<Span> = Vec::new();

        // Session name
        let session_name = self
            .session_id
            .map(|id| {
                if id.len() > 12 {
                    format!("{}...", &id[..12])
                } else {
                    id.to_string()
                }
            })
            .unwrap_or_else(|| t!("header.new_session").to_string());
        left_spans.push(
            Span::raw(format!(" {session_name} "))
                .bold()
                .fg(self.theme.primary),
        );

        // Separator
        left_spans.push(Span::raw("│").fg(self.theme.text_dim));

        // Working directory
        if let Some(dir) = self.working_dir {
            let short_dir = crate::path_display::shorten_path(dir, Self::HEADER_PATH_MAX_LEN);
            left_spans.push(Span::raw(format!(" {short_dir} ")).fg(self.theme.text_dim));
            left_spans.push(Span::raw("│").fg(self.theme.text_dim));
        }

        // Turn count
        if self.turn_count > 0 {
            left_spans.push(
                Span::raw(format!(
                    " {} ",
                    t!("header.turn_count", count = self.turn_count)
                ))
                .fg(self.theme.text_dim),
            );
        }

        // Right side: active worktrees indicator
        if self.active_worktrees > 0 {
            right_spans.push(Span::raw("│").fg(self.theme.text_dim));
            right_spans.push(
                Span::raw(format!(
                    " {} ",
                    t!("header.worktrees", count = self.active_worktrees)
                ))
                .fg(self.theme.secondary),
            );
        }

        // Right side: compaction indicator
        if self.is_compacting {
            right_spans.push(
                Span::raw(format!(" {} ", t!("header.compacting")))
                    .fg(self.theme.warning)
                    .italic(),
            );
        }

        // Right side: fallback model
        if let Some(model) = self.fallback_model {
            right_spans.push(
                Span::raw(format!(" {} ", t!("header.fallback", model = model)))
                    .fg(self.theme.warning),
            );
        }

        // Render left-aligned content
        let left_line = Line::from(left_spans);
        buf.set_line(area.x, area.y, &left_line, area.width);

        // Render right-aligned content
        if !right_spans.is_empty() {
            let right_line = Line::from(right_spans);
            let right_width = right_line.width() as u16;
            let left_width = left_line.width() as u16;
            if left_width + right_width < area.width {
                let right_x = area.x + area.width - right_width;
                buf.set_line(right_x, area.y, &right_line, right_width);
            }
        }
    }
}

#[cfg(test)]
#[path = "header_bar.test.rs"]
mod tests;
