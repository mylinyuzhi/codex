//! Semantic style accessors over the active theme.

use ratatui::style::Color;
use ratatui::style::Style;

use crate::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub(crate) struct UiStyles<'a> {
    theme: &'a Theme,
}

impl<'a> UiStyles<'a> {
    pub(crate) fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    pub(crate) fn primary_border(self) -> Style {
        Style::default().fg(self.theme.primary)
    }

    pub(crate) fn text(self) -> Color {
        self.theme.text
    }

    pub(crate) fn primary(self) -> Color {
        self.theme.primary
    }

    pub(crate) fn dim(self) -> Color {
        self.theme.text_dim
    }

    pub(crate) fn warning(self) -> Color {
        self.theme.warning
    }

    pub(crate) fn thinking(self) -> Color {
        self.theme.thinking
    }

    pub(crate) fn selection_bg(self) -> Color {
        self.theme.selection_bg
    }

    pub(crate) fn selection_fg(self) -> Color {
        self.theme.selection_fg
    }
}
