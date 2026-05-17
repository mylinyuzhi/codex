//! Presentation for read-only information overlays.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::presentation::footer::format_token_count;
use crate::presentation::pager;
use crate::presentation::styles::UiStyles;
use crate::state::AppState;
use crate::state::DiffViewOverlay;

pub(crate) fn diff_view_content(
    d: &DiffViewOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    let all_lines: Vec<&str> = d.diff.lines().collect();
    let window = pager::pager_window(all_lines.len(), d.scroll, 30);
    let visible: String = all_lines
        .get(window.range())
        .unwrap_or_default()
        .iter()
        .map(|line| {
            if line.starts_with('+') && !line.starts_with("+++") {
                format!("  + {}", line.strip_prefix('+').unwrap_or(line))
            } else if line.starts_with('-') && !line.starts_with("---") {
                format!("  - {}", line.strip_prefix('-').unwrap_or(line))
            } else if line.starts_with("@@") {
                format!("  {line}")
            } else {
                format!("    {}", line.strip_prefix(' ').unwrap_or(line))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let position = window.position_suffix();
    (
        t!(
            "dialog.title_diff",
            path = d.path.as_str(),
            position = position.as_str()
        )
        .to_string(),
        format!("{visible}\n\n{}", t!("dialog.scroll_close_hints")),
        styles.primary(),
    )
}

pub(crate) fn context_viz_content(
    state: &AppState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    let used = state.session.context_window_used;
    let total = state.session.context_window_total.max(1);
    let pct = (used * 100) / total;
    let bar_width = 40;
    let filled = ((bar_width * pct / 100).clamp(0, bar_width)) as usize;
    let empty = bar_width as usize - filled;
    let bar = format!("[{}{}] {pct}%", "█".repeat(filled), "░".repeat(empty));

    let tokens = &state.session.token_usage;
    let body = format!(
        "{bar}\n\n{}\n{}\n{}\n\n{}",
        t!(
            "dialog.context_input",
            count = format_token_count(tokens.input_tokens)
        ),
        t!(
            "dialog.context_output",
            count = format_token_count(tokens.output_tokens)
        ),
        t!(
            "dialog.context_cache",
            count = format_token_count(tokens.cache_read_tokens)
        ),
        t!(
            "dialog.context_used",
            used = format_token_count(used as i64),
            total = format_token_count(total as i64)
        ),
    );

    (
        t!("dialog.title_context_window").to_string(),
        format!("{body}\n\n{}", t!("dialog.esc_close")),
        styles.primary(),
    )
}

#[cfg(test)]
#[path = "information.test.rs"]
mod tests;
