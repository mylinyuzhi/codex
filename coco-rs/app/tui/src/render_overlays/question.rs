//! AskUserQuestion overlay renderer.

use ratatui::prelude::Color;

use crate::presentation::request;
use crate::state::QuestionOverlay;
use crate::theme::Theme;

pub(super) fn question_content(q: &QuestionOverlay, theme: &Theme) -> (String, String, Color) {
    request::question_content(q, theme)
}
