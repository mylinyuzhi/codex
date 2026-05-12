//! Filterable-list picker overlay renderers (model, command, session, quick
//! open, export, MCP select).

use coco_types::ModelRole;
use coco_types::ReasoningEffort;
use ratatui::prelude::Color;

use crate::i18n::t;
use crate::state::CommandPaletteOverlay;
use crate::state::ExportOverlay;
use crate::state::McpServerSelectOverlay;
use crate::state::MemoryDialogOverlay;
use crate::state::MemoryDialogScope;
use crate::state::ModelEntry;
use crate::state::ModelPickerOverlay;
use crate::state::QuickOpenOverlay;
use crate::state::SessionBrowserOverlay;
use crate::theme::Theme;

/// Canonical role order — must mirror `update::show::next_role` so the
/// pill order matches Tab/Shift+Tab cycling.
const ROLE_ORDER: [ModelRole; 9] = [
    ModelRole::Main,
    ModelRole::Fast,
    ModelRole::Compact,
    ModelRole::Plan,
    ModelRole::Explore,
    ModelRole::Review,
    ModelRole::HookAgent,
    ModelRole::Memory,
    ModelRole::Subagent,
];

pub(super) fn model_picker_content(
    m: &ModelPickerOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let filter_lower = m.filter.to_lowercase();
    let filtered: Vec<&ModelEntry> = m
        .entries
        .iter()
        .filter(|e| {
            filter_lower.is_empty()
                || e.display_name.to_lowercase().contains(&filter_lower)
                || e.provider_display.to_lowercase().contains(&filter_lower)
        })
        .collect();

    let role_line = render_role_pill(m.role);
    let filter_line = if m.filter.is_empty() {
        t!("dialog.model_picker_type_filter").to_string()
    } else {
        t!("dialog.filter_prefix", text = m.filter.as_str()).to_string()
    };

    let model_lines = render_grouped_models(&filtered, m.selected);
    let footer = render_effort_footer(m, &filtered);
    let hints = t!("dialog.model_picker_hints").to_string();

    let body = if footer.is_empty() {
        format!("{role_line}\n\n{filter_line}\n\n{model_lines}\n\n{hints}")
    } else {
        format!("{role_line}\n\n{filter_line}\n\n{model_lines}\n\n{footer}\n\n{hints}")
    };

    let role_label = role_display(m.role);
    let title = t!("dialog.model_picker_title", role = role_label.as_str()).to_string();
    (title, body, theme.primary)
}

/// Render the role pill row, e.g. `Role:  ▸Main◂  Fast  Compact  ...`.
/// Markers around the active role draw attention without needing colour
/// (the underlying paragraph is single-colour). TS uses tab-bar styling
/// for an equivalent pill in `components/ModelPicker.tsx`.
fn render_role_pill(active: ModelRole) -> String {
    let parts: Vec<String> = ROLE_ORDER
        .iter()
        .map(|r| {
            if *r == active {
                format!("▸{}◂", role_display(*r))
            } else {
                format!(" {} ", role_display(*r))
            }
        })
        .collect();
    format!(
        "{}  {}",
        t!("dialog.model_picker_role_label"),
        parts.join("  ")
    )
}

/// Render the model list with provider headers between sections. The
/// list is already sorted by `(provider_display, display_name)` so a
/// section break is just "previous row's provider != current's".
fn render_grouped_models(entries: &[&ModelEntry], selected: i32) -> String {
    if entries.is_empty() {
        return t!("dialog.model_picker_empty").to_string();
    }
    let mut out: Vec<String> = Vec::with_capacity(entries.len() + 8);
    let mut last_provider: Option<&str> = None;
    for (i, entry) in entries.iter().enumerate() {
        if last_provider != Some(entry.provider_display.as_str()) {
            if !out.is_empty() {
                out.push(String::new()); // blank line between groups
            }
            out.push(entry.provider_display.clone());
            last_provider = Some(entry.provider_display.as_str());
        }
        let marker = if i as i32 == selected {
            "  ❯ "
        } else {
            "    "
        };
        let context = entry
            .context_window
            .map(|w| format!(" · {}", format_context_window(w)))
            .unwrap_or_default();
        let thinking = if entry.supported_efforts.is_empty() {
            String::new()
        } else {
            format!(" · {}", t!("dialog.model_picker_thinking_tag"))
        };
        let current = if entry.is_current_for_role {
            format!("  [{}]", t!("dialog.model_picker_current"))
        } else {
            String::new()
        };
        out.push(format!(
            "{marker}{}{context}{thinking}{current}",
            entry.display_name
        ));
    }
    out.join("\n")
}

/// Format a token count as `1M` / `200K` / `1024`.
fn format_context_window(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        let m = tokens as f64 / 1_000_000.0;
        if (m - m.round()).abs() < 0.05 {
            format!("{}M", m.round() as i64)
        } else {
            format!("{m:.1}M")
        }
    } else if tokens >= 1_000 {
        format!("{}K", tokens / 1_000)
    } else {
        format!("{tokens}")
    }
}

/// Render the thinking-effort footer for the focused model. Returns
/// an empty string when the focused model has no supported levels so
/// the caller can omit the section entirely.
fn render_effort_footer(m: &ModelPickerOverlay, filtered: &[&ModelEntry]) -> String {
    let Some(entry) = filtered.get(m.selected as usize) else {
        return String::new();
    };
    if entry.supported_efforts.is_empty() {
        return String::new();
    }
    let active = m.effort.or(entry.default_effort);
    let chips: Vec<String> = entry
        .supported_efforts
        .iter()
        .map(|e| {
            let label = effort_display(*e);
            if Some(*e) == active {
                format!("▸{label}◂")
            } else {
                format!(" {label} ")
            }
        })
        .collect();
    format!(
        "{}  {}",
        t!("dialog.model_picker_thinking_label"),
        chips.join("  ")
    )
}

/// User-facing role display name. Lookups go through i18n so the
/// translation table owns the wording — ASCII fallbacks aren't
/// hardcoded here.
fn role_display(role: ModelRole) -> String {
    let key = match role {
        ModelRole::Main => "role.main",
        ModelRole::Fast => "role.fast",
        ModelRole::Compact => "role.compact",
        ModelRole::Plan => "role.plan",
        ModelRole::Explore => "role.explore",
        ModelRole::Review => "role.review",
        ModelRole::HookAgent => "role.hook_agent",
        ModelRole::Memory => "role.memory",
        ModelRole::Subagent => "role.subagent",
    };
    t!(key).to_string()
}

/// User-facing effort label. `Auto` shows as "auto" so users don't
/// confuse it with "default" — `Disable` shows as "off".
fn effort_display(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Disable => "off",
        ReasoningEffort::Auto => "auto",
        ReasoningEffort::Minimal => "minimal",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::XHigh => "xhigh",
    }
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

pub(super) fn memory_dialog_content(
    m: &MemoryDialogOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let items: Vec<String> = m
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let marker = if i as i32 == m.selected { "▸ " } else { "  " };
            let scope_tag = match entry.scope {
                MemoryDialogScope::Managed => "[managed]",
                MemoryDialogScope::User => "[user]",
                MemoryDialogScope::Project => "[project]",
                MemoryDialogScope::ProjectLocal => "[project-local]",
                MemoryDialogScope::Subdir => "[subdir]",
            };
            format!("{marker}{scope_tag} {}", entry.label)
        })
        .collect();

    let body = if items.is_empty() {
        t!("dialog.memory_no_files").to_string()
    } else {
        format!(
            "{}\n\n{}\n\n{}",
            t!("dialog.memory_select"),
            items.join("\n"),
            t!("dialog.hints_nav_select_cancel"),
        )
    };

    (t!("dialog.title_memory").to_string(), body, theme.primary)
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
