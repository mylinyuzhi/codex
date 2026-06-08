//! Presentation for request-style prompts.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::PermissionDetail;
use crate::state::PermissionPromptState;
use crate::state::QuestionFocusTarget;
use crate::state::QuestionFooterAction;
use crate::state::QuestionPage;
use crate::state::QuestionPromptState;
use crate::state::RiskLevel;
use crate::state::SubmitAction;
use crate::state::surface_payloads::PermissionAction;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::widgets::ActionRow;
use coco_tui_ui::widgets::ChoiceRow;
use coco_tui_ui::widgets::InputRow;
use coco_tui_ui::widgets::NavTab;
use coco_tui_ui::widgets::QuestionHeader;
use coco_tui_ui::widgets::QuestionNav;
use coco_tui_ui::widgets::QuestionRow;
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
/// i18n + chip truncation + free-text input logic lives here so the widget crate
/// stays domain-free. Replaces the former flat-`String` `question_content`.
pub(crate) fn project_question(q: &QuestionPromptState) -> QuestionView {
    let title = t!("dialog.title_question").to_string();
    let total = q.questions.len();

    if q.questions.is_empty() {
        return QuestionView {
            header: QuestionHeader {
                title,
                chip: None,
                nav: None,
            },
            body: String::new(),
            rows: Vec::new(),
            submit_review: None,
            preview: None,
            footer_actions: Vec::new(),
            hints: t!("dialog.hints_nav_select").to_string(),
        };
    }

    let on_submit = matches!(q.current_question, QuestionPage::Submit);
    let focused_q_idx = match q.current_question {
        QuestionPage::Question(i) => i.min(total - 1),
        QuestionPage::Submit => total - 1,
    };
    let qi = &q.questions[focused_q_idx];

    let (body, rows, preview, submit_review, footer) = if on_submit {
        let rows = vec![
            QuestionRow::Action(ActionRow {
                number: 1,
                label: "Submit answers".to_string(),
                focused: q.focus_target
                    == QuestionFocusTarget::SubmitAction(SubmitAction::SubmitAnswers),
            }),
            QuestionRow::Action(ActionRow {
                number: 2,
                label: "Cancel".to_string(),
                focused: q.focus_target == QuestionFocusTarget::SubmitAction(SubmitAction::Cancel),
            }),
        ];
        (
            String::new(),
            rows,
            None,
            Some(submit_review_text(q)),
            Vec::new(),
        )
    } else {
        let selected_idx = qi.selected.filter(|idx| *idx < qi.options.len());
        let mut rows = qi
            .options
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                let focused = q.focus_target == QuestionFocusTarget::QuestionOption(i);
                let mark = if qi.multi_select {
                    RowMark::Check {
                        checked: qi.checked.contains(&i),
                        focused,
                    }
                } else {
                    RowMark::Radio {
                        selected: Some(i) == selected_idx,
                        focused,
                    }
                };
                QuestionRow::Choice(ChoiceRow {
                    number: i + 1,
                    label: opt.label.clone(),
                    description: opt.description.clone(),
                    mark,
                })
            })
            .collect::<Vec<_>>();
        rows.push(QuestionRow::Input(InputRow {
            number: qi.options.len() + 1,
            label: "Type something.".to_string(),
            value: qi.other_input.value.clone(),
            selected: qi.other_input.committed && !qi.other_input.value.trim().is_empty(),
            focused: q.focus_target == QuestionFocusTarget::OtherInput,
        }));

        let focused_opt = match q.focus_target {
            QuestionFocusTarget::QuestionOption(idx) => qi.options.get(idx),
            QuestionFocusTarget::OtherInput
            | QuestionFocusTarget::QuestionFooter(_)
            | QuestionFocusTarget::SubmitAction(_) => {
                selected_idx.and_then(|idx| qi.options.get(idx))
            }
        };
        let mut footer = vec![ActionRow {
            number: qi.options.len() + 2,
            label: "Chat about this".to_string(),
            focused: q.focus_target
                == QuestionFocusTarget::QuestionFooter(QuestionFooterAction::ChatAboutThis),
        }];
        if q.is_in_plan_mode {
            footer.push(ActionRow {
                number: qi.options.len() + 3,
                label: "Skip interview and plan immediately".to_string(),
                focused: q.focus_target
                    == QuestionFocusTarget::QuestionFooter(QuestionFooterAction::SkipInterview),
            });
        }
        (
            qi.question.clone(),
            rows,
            focused_opt.and_then(|o| o.preview.clone()),
            None,
            footer,
        )
    };

    let hints = if on_submit {
        "Enter to select · Tab/Arrow keys to navigate · Esc to cancel".to_string()
    } else {
        let mut hints = String::from(
            "Enter to select · Tab/Arrow keys to navigate · ctrl+g to edit in Vim · Esc to cancel",
        );
        if qi.multi_select {
            hints.push_str(" · Space to toggle");
        }
        hints
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
        header: QuestionHeader {
            title,
            chip: (total == 1 && !qi.header.is_empty()).then(|| chip(&qi.header)),
            nav,
        },
        body,
        rows,
        submit_review,
        preview,
        footer_actions: footer,
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
        let answer = q.committed_answer_for(item);
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
