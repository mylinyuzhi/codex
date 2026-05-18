//! Presentation for request-style overlays.

use ratatui::prelude::Color;

use super::layout;
use super::styles::UiStyles;
use crate::i18n::t;
use crate::state::OTHER_OPTION_DISPLAY;
use crate::state::OTHER_OPTION_LABEL;
use crate::state::PermissionDetail;
use crate::state::PermissionOverlay;
use crate::state::QuestionFocus;
use crate::state::QuestionItem;
use crate::state::QuestionOverlay;
use crate::state::RiskLevel;
use crate::state::overlay::PermissionAction;

pub(crate) fn permission_content(
    p: &PermissionOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    let detail = permission_detail_for_overlay(p);
    let risk_badge = match p.risk_level {
        Some(RiskLevel::Low) => t!("dialog.risk_low").to_string(),
        Some(RiskLevel::Medium) => t!("dialog.risk_medium").to_string(),
        Some(RiskLevel::High) => t!("dialog.risk_high").to_string(),
        None => String::new(),
    };
    let title = if risk_badge.is_empty() {
        format!(" {} ", p.tool_name)
    } else {
        format!(" {}{risk_badge}", p.tool_name)
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

fn classic_permission_actions(p: &PermissionOverlay) -> String {
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

pub(crate) fn question_content(
    q: &QuestionOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    let title = t!("dialog.title_question").to_string();

    if q.questions.is_empty() {
        return (
            title,
            t!("dialog.hints_nav_select").to_string(),
            styles.primary(),
        );
    }

    let total = q.questions.len();
    let focused_q_idx = match q.focus {
        QuestionFocus::Question(i) => layout::selected_in_bounds(i, total).unwrap_or(0),
        QuestionFocus::ChatAboutThis | QuestionFocus::SkipInterview => 0,
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

    let selected_option_idx = layout::selected_in_bounds(qi.selected, qi.options.len());

    if let Some(opt) = selected_option_idx.and_then(|idx| qi.options.get(idx))
        && let Some(preview) = &opt.preview
    {
        body.push_str("\n— preview —\n");
        body.push_str(preview);
        body.push('\n');
    }

    let focused_is_other = selected_option_idx
        .and_then(|idx| qi.options.get(idx))
        .map(|o| o.label == OTHER_OPTION_LABEL)
        .unwrap_or(false);
    if focused_is_other {
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

    (title, body, styles.primary())
}

fn permission_detail_for_overlay(p: &PermissionOverlay) -> String {
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
    let selected_idx = layout::selected_in_bounds(qi.selected, qi.options.len());
    for (i, opt) in qi.options.iter().enumerate() {
        let i32_i = i as i32;
        let is_focused = on_this_question && selected_idx == Some(i);
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
    format!("{s}▌")
}

#[cfg(test)]
#[path = "request.test.rs"]
mod tests;
