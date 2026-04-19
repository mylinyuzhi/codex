//! Permission overlay content renderer.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::PermissionDetail;
use crate::state::PermissionOverlay;
use crate::state::RiskLevel;
use crate::theme::Theme;

pub(super) fn permission_content(p: &PermissionOverlay, theme: &Theme) -> (String, String, Color) {
    let detail = match &p.detail {
        PermissionDetail::Bash {
            command,
            risk_description,
            working_dir,
        } => {
            let mut s = t!("dialog.perm_command", command = command.as_str()).to_string();
            if let Some(dir) = working_dir {
                s.push_str("\n\n");
                s.push_str(&t!("dialog.perm_directory", path = dir.as_str()));
            }
            if let Some(risk) = risk_description {
                s.push_str("\n\n");
                s.push_str(&t!("dialog.perm_risk_note", risk = risk.as_str()));
            }
            s
        }
        PermissionDetail::FileEdit { path, diff } => {
            let preview = if diff.len() > 500 {
                format!("{}...", &diff[..500])
            } else {
                diff.clone()
            };
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
            format!("{action}: {path}\n\n{content_preview}")
        }
        PermissionDetail::Filesystem { operation, path } => format!("{operation}: {path}"),
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
        } => {
            let mut s = t!("dialog.perm_powershell", command = command.as_str()).to_string();
            if let Some(dir) = working_dir {
                s.push_str("\n\n");
                s.push_str(&t!("dialog.perm_directory", path = dir.as_str()));
            }
            if let Some(risk) = risk_description {
                s.push_str("\n\n");
                s.push_str(&t!("dialog.perm_risk_note", risk = risk.as_str()));
            }
            s
        }
        PermissionDetail::ComputerUse {
            action,
            description,
        } => format!(
            "{}\n\n{description}",
            t!("dialog.perm_computer_use", action = action.as_str())
        ),
        PermissionDetail::Generic { input_preview } => input_preview.clone(),
    };

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

    let actions = if p.show_always_allow {
        t!("dialog.actions_approve_deny_always").to_string()
    } else {
        t!("dialog.actions_approve_deny").to_string()
    };

    // Risk-adaptive border: High uses error color, Medium/Low/None use warning.
    let border = match p.risk_level {
        Some(RiskLevel::High) => theme.error,
        _ => theme.warning,
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
