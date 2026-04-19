//! Filterable-list picker overlay renderers (model, command, session, quick
//! open, export, MCP select).

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::CommandPaletteOverlay;
use crate::state::ExportOverlay;
use crate::state::McpServerSelectOverlay;
use crate::state::ModelPickerOverlay;
use crate::state::QuickOpenOverlay;
use crate::state::SessionBrowserOverlay;
use crate::theme::Theme;

pub(super) fn model_picker_content(
    m: &ModelPickerOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let filter_lower = m.filter.to_lowercase();
    let items: Vec<String> = m
        .models
        .iter()
        .filter(|model| {
            filter_lower.is_empty() || model.label.to_lowercase().contains(&filter_lower)
        })
        .enumerate()
        .map(|(i, model)| {
            let marker = if i as i32 == m.selected { "▸ " } else { "  " };
            let desc = model
                .description
                .as_deref()
                .map(|d| format!(" — {d}"))
                .unwrap_or_default();
            format!("{marker}{}{desc}", model.label)
        })
        .collect();

    let filter_line = if m.filter.is_empty() {
        t!("dialog.type_filter").to_string()
    } else {
        t!("dialog.filter_prefix", text = m.filter.as_str()).to_string()
    };

    (
        t!("dialog.title_model").to_string(),
        format!(
            "{filter_line}\n\n{}\n\n{}",
            items.join("\n"),
            t!("dialog.hints_nav_select_cancel")
        ),
        theme.primary,
    )
}

pub(super) fn command_palette_content(
    cp: &CommandPaletteOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let filter_lower = cp.filter.to_lowercase();
    let items: Vec<String> = cp
        .commands
        .iter()
        .filter(|cmd| filter_lower.is_empty() || cmd.name.to_lowercase().contains(&filter_lower))
        .enumerate()
        .map(|(i, cmd)| {
            let marker = if i as i32 == cp.selected {
                "▸ "
            } else {
                "  "
            };
            let desc = cmd.description.as_deref().unwrap_or("");
            format!("{marker}/{} — {desc}", cmd.name)
        })
        .collect();

    let filter_line = if cp.filter.is_empty() {
        t!("dialog.type_filter_commands").to_string()
    } else {
        t!("dialog.filter_prefix", text = cp.filter.as_str()).to_string()
    };

    (
        t!("dialog.title_commands").to_string(),
        format!(
            "{filter_line}\n\n{}\n\n{}",
            items.join("\n"),
            t!("dialog.hints_nav_select_cancel")
        ),
        theme.accent,
    )
}

pub(super) fn session_browser_content(
    s: &SessionBrowserOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let filter_lower = s.filter.to_lowercase();
    let items: Vec<String> = s
        .sessions
        .iter()
        .filter(|sess| filter_lower.is_empty() || sess.label.to_lowercase().contains(&filter_lower))
        .enumerate()
        .map(|(i, session)| {
            let marker = if i as i32 == s.selected { "▸ " } else { "  " };
            format!(
                "{marker}{} — {}{} — {}",
                session.label,
                session.message_count,
                t!("dialog.sessions_item_suffix"),
                session.created_at
            )
        })
        .collect();

    let body = if items.is_empty() {
        t!("dialog.no_saved_sessions").to_string()
    } else {
        let filter_line = if s.filter.is_empty() {
            t!("dialog.type_filter_sessions").to_string()
        } else {
            t!("dialog.filter_prefix", text = s.filter.as_str()).to_string()
        };
        format!(
            "{filter_line}\n\n{}\n\n{}",
            items.join("\n"),
            t!("dialog.hints_nav_resume_cancel")
        )
    };

    (t!("dialog.title_sessions").to_string(), body, theme.primary)
}

pub(super) fn quick_open_content(q: &QuickOpenOverlay, theme: &Theme) -> (String, String, Color) {
    let filter_line = if q.filter.is_empty() {
        t!("dialog.type_file_name").to_string()
    } else {
        t!("dialog.open_prefix", text = q.filter.as_str()).to_string()
    };

    let items: Vec<String> = q
        .files
        .iter()
        .enumerate()
        .take(15)
        .map(|(i, f)| {
            let marker = if i as i32 == q.selected { "▸ " } else { "  " };
            format!("{marker}{f}")
        })
        .collect();

    (
        t!("dialog.title_quick_open").to_string(),
        format!(
            "{filter_line}\n\n{}\n\n{}",
            items.join("\n"),
            t!("dialog.hints_enter_open_cancel")
        ),
        theme.primary,
    )
}

pub(super) fn export_content(e: &ExportOverlay, theme: &Theme) -> (String, String, Color) {
    let items: Vec<String> = e
        .formats
        .iter()
        .enumerate()
        .map(|(i, fmt)| {
            let marker = if i as i32 == e.selected { "▸ " } else { "  " };
            format!("{marker}{}", fmt.label())
        })
        .collect();

    (
        t!("dialog.title_export").to_string(),
        format!(
            "{}\n\n{}\n\n{}",
            t!("dialog.select_format"),
            items.join("\n"),
            t!("dialog.hints_nav_export_cancel")
        ),
        theme.primary,
    )
}

pub(super) fn mcp_server_select_content(
    ms: &McpServerSelectOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let items: Vec<String> = ms
        .servers
        .iter()
        .map(|s| {
            let check = if s.selected { "[x]" } else { "[ ]" };
            format!(
                "  {check} {} ({})",
                s.name,
                t!("mcp.tools_count", count = s.tool_count)
            )
        })
        .collect();
    (
        t!("dialog.title_select_mcp_servers").to_string(),
        format!(
            "{}\n\n{}",
            t!("dialog.filter_prefix", text = ms.filter.as_str()),
            items.join("\n")
        ),
        theme.accent,
    )
}
