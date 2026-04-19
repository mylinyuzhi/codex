//! Context window visualization overlay renderer.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::render;
use crate::state::AppState;
use crate::theme::Theme;

pub(super) fn context_viz_content(state: &AppState, theme: &Theme) -> (String, String, Color) {
    let used = state.session.context_window_used;
    let total = state.session.context_window_total.max(1);
    let pct = (used * 100) / total;
    let bar_width = 40;
    let filled = (bar_width * pct / 100) as usize;
    let empty = bar_width as usize - filled;
    let bar = format!("[{}{}] {pct}%", "█".repeat(filled), "░".repeat(empty));

    let tokens = &state.session.token_usage;
    let body = format!(
        "{bar}\n\n{}\n{}\n{}\n\n{}",
        t!(
            "dialog.context_input",
            count = render::format_token_count(tokens.input_tokens)
        ),
        t!(
            "dialog.context_output",
            count = render::format_token_count(tokens.output_tokens)
        ),
        t!(
            "dialog.context_cache",
            count = render::format_token_count(tokens.cache_read_tokens)
        ),
        t!(
            "dialog.context_used",
            used = render::format_token_count(used as i64),
            total = render::format_token_count(total as i64)
        ),
    );

    (
        t!("dialog.title_context_window").to_string(),
        format!("{body}\n\n{}", t!("dialog.esc_close")),
        theme.primary,
    )
}
