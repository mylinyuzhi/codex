//! Rewind overlay renderer.
//!
//! TS: `components/MessageSelector.tsx`. Single component with three
//! visible states (pick-list, confirm, summarize-feedback inline) plus
//! a loading spinner. We model each as an explicit phase but render
//! the same content shape.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::rewind::RestoreType;
use crate::state::rewind::RewindOverlay;
use crate::state::rewind::RewindPhase;
use crate::state::rewind::RewindableMessage;
use crate::theme::Theme;
use crate::update_rewind;

pub(super) fn rewind_overlay_content(r: &RewindOverlay, theme: &Theme) -> (String, String, Color) {
    match r.phase {
        RewindPhase::MessageSelect => message_select(r, theme),
        RewindPhase::RestoreOptions => restore_options(r, theme),
        RewindPhase::SummarizeFeedback => summarize_feedback(r, theme),
        RewindPhase::Confirming => confirming(r, theme),
    }
}

fn confirming(r: &RewindOverlay, theme: &Theme) -> (String, String, Color) {
    // TS `MessageSelector.tsx:341-344`: shows a Spinner + "Summarizing…"
    // when an in-flight summarize is pending; otherwise generic
    // "Rewinding..." while file/conversation restore runs.
    let body = if r.pending_summarize.is_some() {
        t!("dialog.rewind_summarizing")
    } else {
        t!("dialog.rewind_in_progress")
    };
    (
        t!("dialog.title_rewind").to_string(),
        body.to_string(),
        theme.accent,
    )
}

fn summarize_feedback(r: &RewindOverlay, theme: &Theme) -> (String, String, Color) {
    // TS `MessageSelector.tsx:107-128` renders an inline input inside
    // the option's `Select` branch. We use a dedicated phase but match
    // TS placeholder + `allowEmptySubmitToCancel` semantics (empty
    // submit cancels, handled in `update_rewind::handle_rewind_confirm`).
    let prompt = t!("dialog.rewind_summarize_prompt");
    let typed = if r.summarize_feedback.is_empty() {
        t!("dialog.rewind_summarize_placeholder").into_owned()
    } else {
        r.summarize_feedback.clone()
    };
    (
        t!("dialog.title_rewind").to_string(),
        format!(
            "{prompt}\n\n  > {typed}\n\n{}",
            t!("dialog.hints_summarize_feedback")
        ),
        theme.accent,
    )
}

fn message_select(r: &RewindOverlay, theme: &Theme) -> (String, String, Color) {
    // Inline empty-state — TS `MessageSelector.tsx:325-327`:
    // `<Text>Nothing to rewind to yet.</Text>` shown when only the
    // synthetic current-prompt row exists (TS `hasMessagesToSelect =
    // messageOptions.length > 1`, line 71).
    if update_rewind::picker_is_empty(r) {
        return (
            t!("dialog.title_rewind").to_string(),
            format!(
                "{}\n\n{}",
                t!("dialog.rewind_no_messages_inline"),
                t!("dialog.esc_close")
            ),
            theme.accent,
        );
    }

    let (start, end) = update_rewind::visible_range(r);
    let items: Vec<String> = r.messages[start..end]
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let global_idx = (start + i) as i32;
            let marker = if global_idx == r.selected { ">" } else { " " };
            // TS `MessageSelector.tsx:591-601` renders the synthetic
            // current-prompt row as italic `(current)` with no
            // timestamp / diff metadata.
            if msg.is_current_prompt {
                return format!("{marker} {}", msg.display_text);
            }
            // TS `MessageSelector.tsx:359-390` renders each row as
            // `pointer · message-text · per-row-diff-stats`.
            let mut line = format!("{marker} {} ({})", msg.display_text, msg.relative_time);
            if r.file_history_enabled
                && let Some(stats_line) = row_diff_stats_line(msg)
            {
                line.push_str(&format!("\n    {stats_line}"));
            }
            line
        })
        .collect();

    let scroll_hint = if r.messages.len() > end - start {
        format!("\n  ({}/{})", r.selected + 1, r.messages.len())
    } else {
        String::new()
    };

    // TS splits the prompt by file-history availability (lines 352-357).
    let prompt_key = if r.file_history_enabled {
        "dialog.rewind_select_with_files"
    } else {
        "dialog.rewind_select_no_files"
    };

    (
        t!("dialog.title_rewind").to_string(),
        format!(
            "{}\n\n{}{scroll_hint}\n\n{}",
            t!(prompt_key),
            items.join("\n"),
            t!("dialog.hints_nav_select_cancel")
        ),
        theme.accent,
    )
}

/// Per-row file-change summary. TS `MessageSelector.tsx:376-387`:
/// single-file rows show `<basename> +X -Y`; multi-file rows show
/// `N files changed +X -Y`; zero-changes shows `No code changes`;
/// no snapshot for the row shows `⚠ No code restore`.
fn row_diff_stats_line(msg: &RewindableMessage) -> Option<String> {
    let stats = match (&msg.diff_stats, msg.can_restore_code) {
        (Some(s), _) => s,
        (None, Some(false)) => return Some(t!("dialog.rewind_diff_no_restore").to_string()),
        // `None, None` = still loading; keep the row single-height.
        (None, _) => return None,
    };
    if stats.files_changed == 0 {
        return Some(t!("dialog.rewind_diff_no_changes").to_string());
    }
    let prefix = match (stats.files_changed, stats.file_paths.first()) {
        (1, Some(p)) => t!("dialog.rewind_diff_file_changed_one", file = basename(p)).to_string(),
        _ => t!(
            "dialog.rewind_diff_files_changed_many",
            count = stats.files_changed
        )
        .to_string(),
    };
    let stats_text = t!(
        "dialog.rewind_diff_stats_short",
        ins = stats.insertions,
        del = stats.deletions
    );
    Some(format!("{prefix} {stats_text}"))
}

/// Extract a path basename in display form. TS uses `path.basename()`;
/// we accept either OS separator and fall back to the full path when no
/// separator is present.
fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit_once(['/', '\\'])
        .map(|(_, last)| last.to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

/// Compose a multi-file label. TS `RestoreCodeConfirmation`
/// (`MessageSelector.tsx:481-523`):
///   1 file:   `<basename>`
///   2 files:  `<basename1> and <basename2>`
///   3+ files: `<basename1> and N-1 other files`
fn file_label(stats: &crate::state::DiffStatsPreview) -> Option<String> {
    let bases: Vec<String> = stats.file_paths.iter().map(|p| basename(p)).collect();
    match bases.as_slice() {
        [] => None,
        [a] => Some(a.clone()),
        [a, b] => Some(t!("dialog.rewind_files_two", a = a.as_str(), b = b.as_str()).to_string()),
        [a, ..] => Some(
            t!(
                "dialog.rewind_files_many",
                first = a.as_str(),
                rest = stats.files_changed - 1
            )
            .to_string(),
        ),
    }
}

fn restore_options(r: &RewindOverlay, theme: &Theme) -> (String, String, Color) {
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

    // TS `MessageSelector.tsx:334-339`: the selected user message is
    // rendered in a left-bordered block with its relative timestamp.
    let message_block = match r.messages.get(r.selected as usize) {
        Some(msg) => format!("  │ {}\n  │ ({})", msg.display_text, msg.relative_time),
        None => String::new(),
    };

    let focused = r
        .available_options
        .get(r.option_selected as usize)
        .cloned()
        .unwrap_or(RestoreType::Nevermind);

    // TS `getRestoreOptionConversationText` (lines 402-414).
    let conv_line = match &focused {
        RestoreType::SummarizeFrom { .. } => t!("dialog.rewind_desc_summarize"),
        RestoreType::SummarizeUpTo { .. } => t!("dialog.rewind_desc_summarize_up_to"),
        RestoreType::Both | RestoreType::ConversationOnly => t!("dialog.rewind_desc_forked"),
        RestoreType::CodeOnly | RestoreType::Nevermind => t!("dialog.rewind_desc_unchanged"),
    };

    // TS `RestoreOptionDescription` (line 442) plus `RestoreCodeConfirmation`
    // (lines 461-543): code-line is absent for summarize options; for
    // Both/CodeOnly it shows the diff stats with file label; for
    // everything else it shows "The code will be unchanged."
    let code_line = match &focused {
        RestoreType::SummarizeFrom { .. } | RestoreType::SummarizeUpTo { .. } => String::new(),
        RestoreType::Both | RestoreType::CodeOnly => match &r.diff_stats {
            Some(stats) if stats.files_changed > 0 => {
                let stats_text = t!(
                    "dialog.rewind_diff_stats_short",
                    ins = stats.insertions,
                    del = stats.deletions
                );
                let label = file_label(stats).unwrap_or_else(|| {
                    t!(
                        "dialog.rewind_diff_files_changed_many",
                        count = stats.files_changed
                    )
                    .to_string()
                });
                format!(
                    "\n{}",
                    t!(
                        "dialog.rewind_desc_code_will_restore",
                        stats = stats_text,
                        label = label.as_str()
                    )
                )
            }
            _ => format!("\n{}", t!("dialog.rewind_desc_code_no_changes")),
        },
        RestoreType::ConversationOnly | RestoreType::Nevermind => {
            format!("\n{}", t!("dialog.rewind_desc_code_unchanged"))
        }
    };

    // TS `MessageSelector.tsx:328-333`: prompt drops "the conversation "
    // when code restore is also being offered.
    let confirm_prompt_key = if r.has_file_changes {
        "dialog.rewind_confirm_prompt"
    } else {
        "dialog.rewind_confirm_prompt_conv"
    };

    // TS line 345-350: warning is only shown when canRestoreCode is true.
    let manual_warning = if r.file_history_enabled && r.has_file_changes {
        format!("\n\n{}", t!("dialog.rewind_manual_warning"))
    } else {
        String::new()
    };

    (
        t!("dialog.title_rewind").to_string(),
        format!(
            "{}\n\n{message_block}\n\n{}\n\n{conv_line}{code_line}{manual_warning}\n\n{}",
            t!(confirm_prompt_key),
            items.join("\n"),
            t!("dialog.hints_nav_confirm_back")
        ),
        theme.accent,
    )
}
