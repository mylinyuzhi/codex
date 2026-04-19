//! Full-screen diff view overlay renderer.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::DiffViewOverlay;
use crate::theme::Theme;

pub(super) fn diff_view_content(d: &DiffViewOverlay, theme: &Theme) -> (String, String, Color) {
    // Show file path header + colored diff lines with scroll offset.
    let all_lines: Vec<&str> = d.diff.lines().collect();
    let total = all_lines.len();
    let offset = (d.scroll as usize).min(total);
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
