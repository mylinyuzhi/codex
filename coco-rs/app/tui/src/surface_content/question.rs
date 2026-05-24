//! AskUserQuestion state content builder.

use ratatui::prelude::Color;

use crate::presentation::request;
use crate::presentation::styles::UiStyles;
use crate::state::QuestionPromptState;

pub(super) fn question_content(
    q: &QuestionPromptState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    request::question_content(q, styles)
}
