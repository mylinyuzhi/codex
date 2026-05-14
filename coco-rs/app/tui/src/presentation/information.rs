//! Presentation for read-only information overlays.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::render;
use crate::state::AppState;
use crate::state::DiffViewOverlay;
use crate::theme::Theme;

pub(crate) fn diff_view_content(d: &DiffViewOverlay, theme: &Theme) -> (String, String, Color) {
    let all_lines: Vec<&str> = d.diff.lines().collect();
    let total = all_lines.len();
    let offset = (d.scroll.max(0) as usize).min(total);
    let visible: String = all_lines
        .iter()
        .skip(offset)
        .take(30)
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

    let position = if total > 0 {
        format!(" [{}/{total}]", offset + 1)
    } else {
        String::new()
    };
    (
        t!(
            "dialog.title_diff",
            path = d.path.as_str(),
            position = position.as_str()
        )
        .to_string(),
        format!("{visible}\n\n{}", t!("dialog.scroll_close_hints")),
        theme.primary,
    )
}

pub(crate) fn context_viz_content(state: &AppState, theme: &Theme) -> (String, String, Color) {
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

#[cfg(test)]
#[path = "information.test.rs"]
mod tests;
