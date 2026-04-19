//! Global search overlay (ripgrep streaming) renderer.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::GlobalSearchOverlay;
use crate::theme::Theme;

pub(super) fn global_search_content(
    g: &GlobalSearchOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let query_line = if g.query.is_empty() {
        t!("dialog.type_search").to_string()
    } else {
        t!("dialog.search_prefix", text = g.query.as_str()).to_string()
    };

    let results: Vec<String> = g
        .results
        .iter()
        .enumerate()
        .take(20)
        .map(|(i, r)| {
            let marker = if i as i32 == g.selected { "▸ " } else { "  " };
            format!("{marker}{}:{} {}", r.file, r.line_number, r.content.trim())
        })
        .collect();

    let status = if g.is_searching {
        format!("\n{}", t!("dialog.searching"))
    } else if g.results.is_empty() && !g.query.is_empty() {
        format!("\n{}", t!("dialog.no_results"))
    } else {
        String::new()
    };

    (
        t!("dialog.title_global_search").to_string(),
        format!(
            "{query_line}{status}\n\n{}\n\n{}",
            results.join("\n"),
            t!("dialog.esc_cancel")
        ),
        theme.primary,
    )
}
