//! AskUserQuestion / structured question overlay renderer.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::QuestionOverlay;
use crate::theme::Theme;

pub(super) fn question_content(q: &QuestionOverlay, theme: &Theme) -> (String, String, Color) {
    let items: Vec<String> = q
        .options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let marker = if i as i32 == q.selected { "▸ " } else { "  " };
            format!("{marker}{opt}")
        })
        .collect();
    (
        t!("dialog.title_question").to_string(),
        format!(
            "{}\n\n{}\n\n{}",
            q.question,
            items.join("\n"),
            t!("dialog.hints_nav_select")
        ),
        theme.primary,
    )
}
