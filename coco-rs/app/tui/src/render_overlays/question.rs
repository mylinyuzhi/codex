//! AskUserQuestion overlay renderer (TS parity:
//! `claude-code/src/components/permissions/AskUserQuestionPermissionRequest/`).
//!
//! Renders the focused element (question OR footer item):
//!   - `[header] N/M` progress chip
//!   - question text
//!   - options as radio (▸) or checkbox ([x]/[ ]) per `multi_select`
//!   - per-option `description` underneath
//!   - focused option's `preview` (when present) appended below
//!   - notes textbox preview (or live "Other" buffer) when populated
//!   - footer items: "Chat about this" (always), "Skip interview"
//!     (plan-mode only) — focus marker `▸` when active
//!
//! Returns the `(title, body, color)` triple consumed by the
//! generic dialog frame in `render_overlays/mod.rs`.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::OTHER_OPTION_DISPLAY;
use crate::state::OTHER_OPTION_LABEL;
use crate::state::QuestionFocus;
use crate::state::QuestionItem;
use crate::state::QuestionOverlay;
use crate::theme::Theme;

pub(super) fn question_content(q: &QuestionOverlay, theme: &Theme) -> (String, String, Color) {
    let title = t!("dialog.title_question").to_string();

    if q.questions.is_empty() {
        return (
            title,
            t!("dialog.hints_nav_select").to_string(),
            theme.primary,
        );
    }

    let total = q.questions.len();
    // Pick which question to highlight in the body. When focus is on a
    // footer item, show whichever question was active most recently
    // (default: first) so the user can still see context.
    let focused_q_idx = match q.focus {
        QuestionFocus::Question(i) => (i as usize).min(total.saturating_sub(1)),
        _ => 0,
    };
    let qi = &q.questions[focused_q_idx];

    let mut body = String::new();

    if total > 1 {
        body.push_str(&format!(
            "[{}] {}/{}\n",
            chip(&qi.header),
            focused_q_idx + 1,
            total
        ));
    } else if !qi.header.is_empty() {
        body.push_str(&format!("[{}]\n", chip(&qi.header)));
    }
    if !qi.question.is_empty() {
        body.push_str(&qi.question);
        body.push_str("\n\n");
    } else {
        body.push('\n');
    }

    body.push_str(&render_options(qi, q.focus));

    if let Some(opt) = qi.options.get(qi.selected as usize)
        && let Some(preview) = &opt.preview
    {
        body.push_str("\n— preview —\n");
        body.push_str(preview);
        body.push('\n');
    }

    // Notes / Other-buffer preview.
    let focused_is_other = qi
        .options
        .get(qi.selected as usize)
        .map(|o| o.label == OTHER_OPTION_LABEL)
        .unwrap_or(false);
    if focused_is_other {
        // Live edit buffer for the Other option (typed answer).
        body.push_str(&format!("\nyour answer: {}", display_buffer(&qi.notes)));
        body.push('\n');
    } else if !qi.notes.is_empty() {
        body.push_str(&format!("\nnotes: {}\n", qi.notes));
    }

    body.push_str(&render_footer(q));

    body.push('\n');
    body.push_str(t!("dialog.hints_nav_select").as_ref());
    if total > 1 {
        body.push_str("  Tab: next question / footer");
    }
    if qi.multi_select {
        body.push_str("  Space: toggle");
    }

    (title, body, theme.primary)
}

fn chip(s: &str) -> String {
    if s.chars().count() > 20 {
        let truncated: String = s.chars().take(19).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

fn render_options(qi: &QuestionItem, focus: QuestionFocus) -> String {
    let mut out = String::new();
    let on_this_question = matches!(focus, QuestionFocus::Question(_));
    for (i, opt) in qi.options.iter().enumerate() {
        let i32_i = i as i32;
        let is_focused = on_this_question && i32_i == qi.selected;
        let display_label = if opt.label == OTHER_OPTION_LABEL {
            OTHER_OPTION_DISPLAY
        } else {
            opt.label.as_str()
        };
        let marker = if qi.multi_select {
            let checked = qi.checked.contains(&i32_i);
            let cursor = if is_focused { ">" } else { " " };
            format!("{cursor} [{}]", if checked { "x" } else { " " })
        } else if is_focused {
            "▸ ".into()
        } else {
            "  ".into()
        };
        out.push_str(&format!("{marker} {display_label}\n"));
        if !opt.description.is_empty() {
            out.push_str(&format!("    {}\n", opt.description));
        }
    }
    out
}

fn render_footer(q: &QuestionOverlay) -> String {
    let mut out = String::from("\n");
    let chat_focused = matches!(q.focus, QuestionFocus::ChatAboutThis);
    let skip_focused = matches!(q.focus, QuestionFocus::SkipInterview);
    let marker = |focused: bool| if focused { "▸ " } else { "  " };
    out.push_str(&format!("{}Chat about this\n", marker(chat_focused)));
    if q.is_in_plan_mode {
        out.push_str(&format!(
            "{}Skip interview and plan immediately\n",
            marker(skip_focused)
        ));
    }
    out
}

fn display_buffer(s: &str) -> String {
    // Render a trailing cursor so the user sees the insertion point
    // even when the buffer is empty.
    format!("{s}▌")
}
