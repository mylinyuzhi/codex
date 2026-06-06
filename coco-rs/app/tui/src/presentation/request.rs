//! Presentation for request-style prompts.

use ratatui::prelude::Color;

use super::layout;
use crate::i18n::t;
use crate::state::PermissionDetail;
use crate::state::PermissionPromptState;
use crate::state::QuestionFocus;
use crate::state::QuestionPromptState;
use crate::state::RiskLevel;
use crate::state::surface_payloads::PermissionAction;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::widgets::FooterAction;
use coco_tui_ui::widgets::NavTab;
use coco_tui_ui::widgets::OptionRow;
use coco_tui_ui::widgets::QuestionNav;
use coco_tui_ui::widgets::QuestionView;
use coco_tui_ui::widgets::RowMark;
use coco_tui_ui::widgets::SubmitNavTab;

pub(crate) fn permission_content(
    p: &PermissionPromptState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    let detail = permission_detail_for_prompt(p);
    let risk_badge = match p.risk_level {
        Some(RiskLevel::Low) => t!("dialog.risk_low").to_string(),
        Some(RiskLevel::Medium) => t!("dialog.risk_medium").to_string(),
        Some(RiskLevel::High) => t!("dialog.risk_high").to_string(),
        None => String::new(),
    };
    // Cross-process teammate requests carry a worker badge — suffix the
    // title with `· @name` so the leader sees who is asking (TS
    // `PermissionRequestTitle.tsx:32`). The text surface is monochrome;
    // the badge color is carried on the event for styled / SDK consumers.
    let worker_suffix = p
        .worker_badge
        .as_ref()
        .map(|b| format!(" · @{}", b.name))
        .unwrap_or_default();
    let title = if risk_badge.is_empty() {
        format!(" {}{worker_suffix} ", p.tool_name)
    } else {
        format!(" {}{risk_badge}{worker_suffix}", p.tool_name)
    };

    let classifier_line = if let Some(rule) = &p.classifier_auto_approved {
        t!("chat.auto_approved", rule = rule.as_str()).to_string()
    } else if p.classifier_checking {
        format!("\n{}", t!("dialog.checking"))
    } else {
        String::new()
    };

    let actions = if let Some(choices) = &p.choices {
        let selected = p.selected_choice.min(choices.len().saturating_sub(1));
        let mut lines = String::new();
        for (idx, choice) in choices.iter().enumerate() {
            let marker = if idx == selected { "▸ " } else { "  " };
            lines.push_str(&format!("{marker}{}\n", choice.label));
            if let Some(desc) = &choice.description {
                lines.push_str(&format!("    {desc}\n"));
            }
        }
        lines.push_str(t!("dialog.hints_nav_select").as_ref());
        lines
    } else {
        classic_permission_actions(p)
    };

    let border = match p.risk_level {
        Some(RiskLevel::High) => styles.error(),
        _ => styles.warning(),
    };

    (
        title,
        format!(
            "{}{classifier_line}\n\n{detail}\n\n{actions}",
            p.description
        ),
        border,
    )
}

fn classic_permission_actions(p: &PermissionPromptState) -> String {
    let selected = p
        .selected_choice
        .min(p.classic_action_count().saturating_sub(1));
    let mut lines = format!("{}:\n", t!("dialog.actions_heading"));
    for idx in 0..p.classic_action_count() {
        let marker = if idx == selected { "▸ " } else { "  " };
        let label = match p.classic_action_at(idx) {
            PermissionAction::ApproveOnce => t!("dialog.action_approve_once").to_string(),
            PermissionAction::AlwaysAllow => t!(
                "dialog.action_always_allow_session",
                tool = p.tool_name.as_str()
            )
            .to_string(),
            PermissionAction::Deny => t!("dialog.action_deny").to_string(),
        };
        lines.push_str(&format!("{marker}{label}\n"));
    }
    lines.push_str(t!("dialog.hints_nav_select").as_ref());
    lines.push_str("  ");
    lines.push_str(t!("dialog.hints_permission_shortcuts").as_ref());
    lines
}

/// Project the domain [`QuestionPromptState`] into the pure, area-based
/// [`QuestionView`] rendered by `coco_tui_ui::widgets::QuestionWidget`. All
/// i18n + chip truncation + Other-composer logic lives here so the widget crate
/// stays domain-free. Replaces the former flat-`String` `question_content`.
pub(crate) fn project_question(q: &QuestionPromptState) -> QuestionView {
    let title = t!("dialog.title_question").to_string();
    let total = q.questions.len();

    if q.questions.is_empty() {
        return QuestionView {
            title,
            chip: None,
            nav: None,
            prompt: String::new(),
            rows: Vec::new(),
            preview: None,
            composer: None,
            footer: Vec::new(),
            hints: t!("dialog.hints_nav_select").to_string(),
        };
    }

    let focused_q_idx = match q.focus {
        QuestionFocus::Question(i) => layout::selected_in_bounds(i, total).unwrap_or(0),
        QuestionFocus::Submit | QuestionFocus::ChatAboutThis | QuestionFocus::SkipInterview => 0,
    };
    let on_submit = matches!(q.focus, QuestionFocus::Submit);
    let on_this_question = matches!(q.focus, QuestionFocus::Question(_));
    let qi = &q.questions[focused_q_idx];
    let selected_idx = layout::selected_in_bounds(qi.selected, qi.options.len());

    let rows = qi
        .options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let focused = on_this_question && selected_idx == Some(i);
            let mark = if qi.multi_select {
                RowMark::Check {
                    checked: qi.checked.contains(&(i as i32)),
                    focused,
                }
            } else {
                RowMark::Radio { focused }
            };
            OptionRow {
                number: i + 1,
                label: opt.label.clone(),
                description: opt.description.clone(),
                mark,
            }
        })
        .collect();

    // Preview + Other composer track the focused option (only while a question,
    // not a footer item, is focused).
    let focused_opt = selected_idx
        .filter(|_| on_this_question)
        .and_then(|idx| qi.options.get(idx));
    let preview = focused_opt.and_then(|o| o.preview.clone());
    let composer = (on_this_question && qi.is_editing()).then(|| qi.notes.clone());

    let mut footer = vec![FooterAction {
        label: "Chat about this".to_string(),
        focused: matches!(q.focus, QuestionFocus::ChatAboutThis),
    }];
    if q.is_in_plan_mode {
        footer.push(FooterAction {
            label: "Skip interview and plan immediately".to_string(),
            focused: matches!(q.focus, QuestionFocus::SkipInterview),
        });
    }

    let hints = if on_submit {
        "↑/↓: choose   Enter: confirm   ←/→: back to questions".to_string()
    } else {
        let mut hints = t!("dialog.hints_nav_select").to_string();
        if !qi.options.is_empty() {
            hints.push_str(&format!("  1-{}: pick", qi.options.len().min(9)));
        }
        // ←/→ switch questions + the Submit tab (codex `move_question`); Tab
        // reaches the footer actions, so surface the escape hatch there.
        if total > 1 {
            hints.push_str("  ←/→: question / submit");
        }
        if qi.multi_select {
            hints.push_str("  Space: toggle");
        }
        hints.push_str(if q.is_in_plan_mode {
            "  Tab: chat / skip"
        } else {
            "  Tab: chat about this"
        });
        hints
    };

    // On the Submit tab, replace the option list with a read-only review of
    // every answer plus a Submit / Cancel confirmation list (TS "Review your
    // answers" → "Ready to submit your answers?").
    let (prompt, rows, preview, composer) = if on_submit {
        let confirm_rows = vec![
            OptionRow {
                number: 1,
                label: "Submit answers".to_string(),
                description: String::new(),
                mark: RowMark::Radio {
                    focused: q.submit_selected == 0,
                },
            },
            OptionRow {
                number: 2,
                label: "Cancel".to_string(),
                description: String::new(),
                mark: RowMark::Radio {
                    focused: q.submit_selected == 1,
                },
            },
        ];
        (submit_review_text(q), confirm_rows, None, None)
    } else {
        (qi.question.clone(), rows, preview, composer)
    };

    // >1 question → a nav strip (every header + the trailing Submit tab, current
    // highlighted) replaces the single-question chip; 1 question keeps `[chip]`.
    let nav = (total > 1).then(|| QuestionNav {
        tabs: q
            .questions
            .iter()
            .map(|item| NavTab {
                header: chip(&item.header),
                answered: q.question_has_answer(item),
            })
            .collect(),
        current: focused_q_idx,
        submit: Some(SubmitNavTab {
            focused: on_submit,
            ready: q.all_answered(),
        }),
    });

    QuestionView {
        title,
        chip: (total == 1 && !qi.header.is_empty()).then(|| chip(&qi.header)),
        nav,
        prompt,
        rows,
        preview,
        composer,
        footer,
        hints,
    }
}

/// Read-only "Review your answers" body shown above the Submit/Cancel list when
/// the Submit tab is focused. Mirrors the TS review screen (warning when not all
/// answered, `● question → answer` per row, then the "Ready to submit?" prompt).
fn submit_review_text(q: &QuestionPromptState) -> String {
    let mut out = String::from("Review your answers");
    if !q.all_answered() {
        out.push_str("\n\n⚠ You have not answered all questions");
    }
    for item in &q.questions {
        let answer = q.peek_answer_for(item);
        let answer = answer.trim();
        let answer = if answer.is_empty() {
            "(unanswered)"
        } else {
            answer
        };
        out.push_str(&format!("\n\n● {}\n   → {answer}", item.question));
    }
    out.push_str("\n\nReady to submit your answers?");
    out
}

fn permission_detail_for_prompt(p: &PermissionPromptState) -> String {
    if matches!(p.detail, PermissionDetail::Generic { .. }) {
        return p.display_input.as_display_str().to_string();
    }
    permission_detail(&p.detail)
}

fn permission_detail(detail: &PermissionDetail) -> String {
    match detail {
        PermissionDetail::Bash {
            command,
            risk_description,
            working_dir,
        } => shell_detail(
            t!("dialog.perm_command", command = command.as_str()).to_string(),
            risk_description,
            working_dir,
        ),
        PermissionDetail::FileEdit { path, diff } => {
            let preview = truncate_preview(diff, 500);
            format!(
                "{}\n\n{preview}",
                t!("dialog.perm_file", path = path.as_str())
            )
        }
        PermissionDetail::FileWrite {
            path,
            content_preview,
            is_new_file,
        } => {
            let action = if *is_new_file {
                t!("dialog.perm_create")
            } else {
                t!("dialog.perm_overwrite")
            };
            let file_line = t!("dialog.perm_file", path = path.as_str());
            let action_label = t!("dialog.perm_action_label");
            format!("{action_label}: {action}\n{file_line}\n\n{content_preview}")
        }
        PermissionDetail::Filesystem { operation, path } => t!(
            "dialog.perm_filesystem",
            operation = operation.as_str(),
            path = path.as_str()
        )
        .to_string(),
        PermissionDetail::WebFetch { url, method } => t!(
            "dialog.perm_web",
            method = method.as_str(),
            url = url.as_str()
        )
        .to_string(),
        PermissionDetail::Skill {
            skill_name,
            skill_description,
        } => {
            let desc = skill_description.as_deref().unwrap_or("");
            format!(
                "{}\n{desc}",
                t!("dialog.perm_skill", name = skill_name.as_str())
            )
        }
        PermissionDetail::SedEdit {
            path,
            pattern,
            replacement,
        } => t!(
            "dialog.perm_sed",
            path = path.as_str(),
            pattern = pattern.as_str(),
            replacement = replacement.as_str()
        )
        .to_string(),
        PermissionDetail::NotebookEdit {
            path,
            cell_id,
            change_preview,
        } => format!(
            "{}\n\n{change_preview}",
            t!(
                "dialog.perm_notebook",
                path = path.as_str(),
                cell = cell_id.as_str()
            )
        ),
        PermissionDetail::McpTool {
            server_name,
            tool_name,
            input_preview,
        } => format!(
            "{}\n\n{input_preview}",
            t!(
                "dialog.perm_mcp",
                server = server_name.as_str(),
                tool = tool_name.as_str()
            )
        ),
        PermissionDetail::PowerShell {
            command,
            risk_description,
            working_dir,
        } => shell_detail(
            t!("dialog.perm_powershell", command = command.as_str()).to_string(),
            risk_description,
            working_dir,
        ),
        PermissionDetail::ComputerUse {
            action,
            description,
        } => format!(
            "{}\n\n{description}",
            t!("dialog.perm_computer_use", action = action.as_str())
        ),
        PermissionDetail::Generic { input_preview } => input_preview.clone(),
    }
}

fn shell_detail(
    mut detail: String,
    risk_description: &Option<String>,
    working_dir: &Option<String>,
) -> String {
    if let Some(dir) = working_dir {
        detail.push_str("\n\n");
        detail.push_str(&t!("dialog.perm_directory", path = dir.as_str()));
    }
    if let Some(risk) = risk_description {
        detail.push_str("\n\n");
        detail.push_str(&t!("dialog.perm_risk_note", risk = risk.as_str()));
    }
    detail
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx == max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

/// Max display width of a question's `header` chip. Mirrors the tool schema's
/// `header` description and TS `ASK_USER_QUESTION_TOOL_CHIP_WIDTH = 12`.
const CHIP_MAX_CHARS: usize = 12;

fn chip(s: &str) -> String {
    if s.chars().count() > CHIP_MAX_CHARS {
        let truncated: String = s.chars().take(CHIP_MAX_CHARS - 1).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
#[path = "request.test.rs"]
mod tests;
