//! AskUserQuestion overlay content builder.

use ratatui::prelude::Color;

use crate::presentation::request;
use crate::presentation::styles::UiStyles;
use crate::state::QuestionOverlay;

pub(super) fn question_content(
    q: &QuestionOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    request::question_content(q, styles)
}
