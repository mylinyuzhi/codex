//! Rewind overlay renderer — two-phase (message select → restore options).
//!
//! TS: MessageSelector.tsx — shows a list of user messages, then restore
//! type choices with an optional diff preview.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::rewind::RewindOverlay;
use crate::state::rewind::RewindPhase;
use crate::theme::Theme;
use crate::update_rewind;

pub(super) fn rewind_overlay_content(r: &RewindOverlay, theme: &Theme) -> (String, String, Color) {
    match r.phase {
        RewindPhase::MessageSelect => message_select(r, theme),
        RewindPhase::RestoreOptions => restore_options(r, theme),
        RewindPhase::Confirming => (
            t!("dialog.title_rewind").to_string(),
            t!("dialog.rewind_in_progress").to_string(),
            theme.accent,
        ),
    }
}

fn message_select(r: &RewindOverlay, theme: &Theme) -> (String, String, Color) {
    let (start, end) = update_rewind::visible_range(r);
    let items: Vec<String> = r.messages[start..end]
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let global_idx = (start + i) as i32;
            let marker = if global_idx == r.selected { ">" } else { " " };
            format!("{marker} {} — {}", msg.turn_label, msg.display_text)
        })
        .collect();

    let scroll_hint = if r.messages.len() > end - start {
        format!("\n  ({}/{})", r.selected + 1, r.messages.len())
    } else {
        String::new()
    };

    (
        t!("dialog.title_rewind").to_string(),
        format!(
            "{}\n\n{}{scroll_hint}\n\n{}",
            t!("dialog.rewind_select"),
            items.join("\n"),
            t!("dialog.hints_nav_select_cancel")
        ),
        theme.accent,
    )
}

fn restore_options(r: &RewindOverlay, theme: &Theme) -> (String, String, Color) {
    let msg_label = r
        .messages
        .get(r.selected as usize)
        .map(|m| m.turn_label.as_str())
        .unwrap_or("?");

    let items: Vec<String> = r
        .available_options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let marker = if i as i32 == r.option_selected {
                ">"
            } else {
                " "
            };
            format!("{marker} {}", opt.label())
        })
        .collect();

    let diff_info = if let Some(ref stats) = r.diff_stats {
        format!(
            "\n\n{}",
            t!(
                "dialog.rewind_diff_stats",
                files = stats.files_changed,
                ins = stats.insertions,
                del = stats.deletions
            )
        )
    } else {
        String::new()
    };

    (
        t!("dialog.title_rewind").to_string(),
        format!(
            "{}\n\n{}{diff_info}\n\n{}",
            t!("dialog.rewind_to", label = msg_label),
            items.join("\n"),
            t!("dialog.hints_nav_confirm_back")
        ),
        theme.accent,
    )
}
