//! AskUserQuestion state content builder.

use ratatui::prelude::Color;

use crate::presentation::request;
use crate::state::QuestionPromptState;
use coco_tui_ui::style::UiStyles;

pub(super) fn question_content(
    q: &QuestionPromptState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    request::question_content(q, styles)
}
