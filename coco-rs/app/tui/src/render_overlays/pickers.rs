//! Filterable-list picker overlay renderers (model, command, session, quick
//! open, export, MCP select).

use coco_types::ModelRole;
use coco_types::ReasoningEffort;
use ratatui::prelude::*;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;

use crate::i18n::t;
use crate::state::CommandPaletteOverlay;
use crate::state::ExportOverlay;
use crate::state::McpServerSelectOverlay;
use crate::state::MemoryDialogOverlay;
use crate::state::MemoryDialogScope;
use crate::state::ModelEntry;
use crate::state::ModelPickerOverlay;
use crate::state::ProviderUnavailableReason;
use crate::state::QuickOpenOverlay;
use crate::state::SessionBrowserOverlay;
use crate::theme::Theme;

/// Canonical role order — must mirror `update::show::next_role` so the
/// pill order matches Tab/Shift+Tab cycling.
const ROLE_ORDER: [ModelRole; 8] = [
    ModelRole::Main,
    ModelRole::Fast,
    ModelRole::Plan,
    ModelRole::Explore,
    ModelRole::Review,
    ModelRole::HookAgent,
    ModelRole::Memory,
    ModelRole::Subagent,
];

const MODEL_PICKER_MIN_WIDTH: u16 = 60;
const MODEL_PICKER_MAX_WIDTH: u16 = 112;
const MODEL_PICKER_MIN_HEIGHT: u16 = 18;
const MODEL_PICKER_MAX_HEIGHT: u16 = 32;

enum ModelPickerRow<'a> {
    Blank,
    Header(&'a str),
    Entry {
        filtered_index: usize,
        entry: &'a ModelEntry,
    },
}

pub(super) fn render_model_picker(
    frame: &mut Frame,
    area: Rect,
    m: &ModelPickerOverlay,
    theme: &Theme,
) {
    let role_label = role_display(m.role);
    let title = t!("dialog.model_picker_title", role = role_label.as_str()).to_string();
    let overlay_area = model_picker_area(area);

    frame.render_widget(Clear, overlay_area);

    let inner_width = overlay_area.width.saturating_sub(2) as usize;
    let inner_height = overlay_area.height.saturating_sub(2) as usize;
    let lines = render_model_picker_lines(m, theme, inner_width, inner_height);
    let content = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(theme.primary)),
    );

    frame.render_widget(content, overlay_area);
}

fn model_picker_area(area: Rect) -> Rect {
    let max_width = area.width.saturating_sub(2).max(1);
    let max_height = area.height.saturating_sub(2).max(1);
    let width = clamp_overlay_len(
        area.width.saturating_mul(70) / 100,
        MODEL_PICKER_MIN_WIDTH,
        MODEL_PICKER_MAX_WIDTH,
        max_width,
    );
    let height = clamp_overlay_len(
        area.height.saturating_mul(80) / 100,
        MODEL_PICKER_MIN_HEIGHT,
        MODEL_PICKER_MAX_HEIGHT,
        max_height,
    );
    area.centered(Constraint::Length(width), Constraint::Length(height))
}

fn clamp_overlay_len(preferred: u16, min: u16, max: u16, available: u16) -> u16 {
    let upper = max.min(available);
    let lower = min.min(upper);
    preferred.clamp(lower, upper)
}

fn render_model_picker_lines(
    m: &ModelPickerOverlay,
    theme: &Theme,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    let filtered = filtered_entries(m);
    let rows = model_rows(&filtered);
    let reserved = 8usize;
    let list_height = height.saturating_sub(reserved).max(1);
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, ModelPickerRow::Entry { filtered_index, .. } if *filtered_index == m.selected as usize))
        .unwrap_or(0);
    let start = visible_start(selected_row, rows.len(), list_height);
    let end = (start + list_height).min(rows.len());

    let mut lines = Vec::with_capacity(height.max(1));
    lines.push(pad_line(render_role_tabs(m.role, theme), width, None));
    lines.push(blank_line(width));
    lines.push(pad_line(render_filter_line(m, theme), width, None));
    lines.push(blank_line(width));
    if rows.is_empty() {
        lines.push(pad_line(
            Line::from(Span::raw(t!("dialog.model_picker_empty").to_string()).fg(theme.text_dim)),
            width,
            None,
        ));
    } else {
        for row in rows.iter().take(end).skip(start) {
            lines.push(render_model_row(row, m.selected as usize, theme, width));
        }
    }
    while lines.len() < height.saturating_sub(4) {
        lines.push(blank_line(width));
    }
    lines.truncate(height.saturating_sub(4));
    lines.push(blank_line(width));
    lines.push(pad_line(
        render_effort_line(m, &filtered, theme),
        width,
        None,
    ));
    lines.push(blank_line(width));
    lines.push(pad_line(
        Line::from(Span::raw(t!("dialog.model_picker_hints").to_string()).fg(theme.text_dim)),
        width,
        None,
    ));
    lines.truncate(height);
    lines
}

fn filtered_entries(m: &ModelPickerOverlay) -> Vec<&ModelEntry> {
    let filter_lower = m.filter.to_lowercase();
    m.entries
        .iter()
        .filter(|e| {
            filter_lower.is_empty()
                || e.display_name.to_lowercase().contains(&filter_lower)
                || e.provider_display.to_lowercase().contains(&filter_lower)
        })
        .collect()
}

fn model_rows<'a>(entries: &'a [&'a ModelEntry]) -> Vec<ModelPickerRow<'a>> {
    let mut rows = Vec::with_capacity(entries.len() + 8);
    let mut last_provider: Option<&str> = None;
    for (filtered_index, entry) in entries.iter().enumerate() {
        if last_provider != Some(entry.provider_display.as_str()) {
            if !rows.is_empty() {
                rows.push(ModelPickerRow::Blank);
            }
            rows.push(ModelPickerRow::Header(entry.provider_display.as_str()));
            last_provider = Some(entry.provider_display.as_str());
        }
        rows.push(ModelPickerRow::Entry {
            filtered_index,
            entry,
        });
    }
    rows
}

fn visible_start(selected_row: usize, row_count: usize, height: usize) -> usize {
    if row_count <= height {
        return 0;
    }
    let centered = selected_row.saturating_sub(height / 2);
    centered.min(row_count.saturating_sub(height))
}

fn render_role_tabs(active: ModelRole, theme: &Theme) -> Line<'static> {
    let mut spans = vec![
        Span::raw(t!("dialog.model_picker_role_label").to_string()).fg(theme.text_dim),
        Span::raw("  "),
    ];
    for (idx, role) in ROLE_ORDER.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
        }
        let label = role_display(*role);
        if *role == active {
            spans.push(
                Span::raw(format!("▸{label}◂"))
                    .fg(theme.selection_fg)
                    .bg(theme.selection_bg)
                    .bold(),
            );
        } else {
            spans.push(Span::raw(format!(" {label} ")).fg(theme.text));
        }
    }
    Line::from(spans)
}

fn render_filter_line(m: &ModelPickerOverlay, theme: &Theme) -> Line<'static> {
    if m.filter.is_empty() {
        Line::from(Span::raw(t!("dialog.model_picker_type_filter").to_string()).fg(theme.text_dim))
    } else {
        Line::from(vec![
            Span::raw(t!("dialog.filter_prefix", text = "").to_string()).fg(theme.text_dim),
            Span::raw(m.filter.clone()).fg(theme.text),
        ])
    }
}

fn render_model_row(
    row: &ModelPickerRow<'_>,
    selected: usize,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    match row {
        ModelPickerRow::Blank => blank_line(width),
        ModelPickerRow::Header(provider) => pad_line(
            Line::from(Span::raw((*provider).to_string()).fg(theme.primary).bold()),
            width,
            None,
        ),
        ModelPickerRow::Entry {
            filtered_index,
            entry,
        } => {
            let is_selected = *filtered_index == selected;
            let bg = entry.is_current_for_role.then_some(theme.selection_bg);
            let text_fg = if entry.unavailable_reasons.is_empty() {
                theme.text
            } else {
                theme.text_dim
            };
            let mut spans = Vec::new();
            if is_selected {
                spans.push(Span::raw("  ").bg_opt(bg));
                spans.push(Span::raw("❯ ").fg(theme.selection_fg).bg_opt(bg).bold());
            } else {
                spans.push(Span::raw("    ").bg_opt(bg));
            }
            let name = Span::raw(entry.display_name.clone()).fg(text_fg).bg_opt(bg);
            spans.push(if is_selected { name.bold() } else { name });
            if let Some(context_window) = entry.context_window {
                spans.push(
                    Span::raw(format!(" · {}", format_context_window(context_window)))
                        .fg(theme.text_dim)
                        .bg_opt(bg),
                );
            }
            if !entry.unavailable_reasons.is_empty() {
                spans.push(
                    Span::raw(format!(" · {}", t!("dialog.model_picker_unavailable_tag")))
                        .fg(theme.warning)
                        .bg_opt(bg)
                        .bold(),
                );
            }
            if !entry.supported_efforts.is_empty() {
                spans.push(
                    Span::raw(format!(" · {}", t!("dialog.model_picker_thinking_tag")))
                        .fg(theme.thinking)
                        .bg_opt(bg),
                );
            }
            if entry.is_current_for_role {
                spans.push(
                    Span::raw(format!("  [{}]", t!("dialog.model_picker_current")))
                        .fg(theme.selection_fg)
                        .bg_opt(bg)
                        .bold(),
                );
            }
            pad_line(Line::from(spans), width, bg)
        }
    }
}

fn render_effort_line(
    m: &ModelPickerOverlay,
    filtered: &[&ModelEntry],
    theme: &Theme,
) -> Line<'static> {
    let Some(entry) = filtered.get(m.selected as usize) else {
        return Line::from(
            Span::raw(t!("dialog.model_picker_thinking_label").to_string()).fg(theme.text_dim),
        );
    };
    if let Some(summary) = unavailable_summary(&entry.unavailable_reasons) {
        return Line::from(vec![
            Span::raw(t!("dialog.model_picker_unavailable_label").to_string())
                .fg(theme.warning)
                .bold(),
            Span::raw("  "),
            Span::raw(summary).fg(theme.text_dim),
        ]);
    }
    let mut spans = vec![
        Span::raw(t!("dialog.model_picker_thinking_label").to_string()).fg(theme.text_dim),
        Span::raw("  "),
    ];
    if entry.supported_efforts.is_empty() {
        spans.push(
            Span::raw(t!("dialog.model_picker_thinking_unavailable").to_string())
                .fg(theme.text_dim),
        );
        return Line::from(spans);
    }
    let active = m.effort.or(entry.default_effort);
    for (idx, effort) in entry.supported_efforts.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
        }
        let label = effort_display(*effort);
        if Some(*effort) == active {
            spans.push(
                Span::raw(format!("▸{label}◂"))
                    .fg(theme.selection_fg)
                    .bg(theme.selection_bg)
                    .bold(),
            );
        } else {
            spans.push(Span::raw(format!(" {label} ")).fg(theme.text));
        }
    }
    Line::from(spans)
}

trait SpanBgOpt<'a> {
    fn bg_opt(self, bg: Option<Color>) -> Span<'a>;
}

impl<'a> SpanBgOpt<'a> for Span<'a> {
    fn bg_opt(self, bg: Option<Color>) -> Span<'a> {
        if let Some(bg) = bg { self.bg(bg) } else { self }
    }
}

fn pad_line(mut line: Line<'static>, width: usize, bg: Option<Color>) -> Line<'static> {
    let used = line.width();
    if used < width {
        let pad = " ".repeat(width - used);
        let span = if let Some(bg) = bg {
            Span::raw(pad).bg(bg)
        } else {
            Span::raw(pad)
        };
        line.spans.push(span);
    }
    line
}

fn blank_line(width: usize) -> Line<'static> {
    Line::from(Span::raw(" ".repeat(width)))
}

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

/// Render the role pill row, e.g. `Role:  ▸Main◂  Fast  Plan  ...`.
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
        let unavailable = if entry.unavailable_reasons.is_empty() {
            String::new()
        } else {
            format!(" · {}", t!("dialog.model_picker_unavailable_tag"))
        };
        let current = if entry.is_current_for_role {
            format!("  [{}]", t!("dialog.model_picker_current"))
        } else {
            String::new()
        };
        out.push(format!(
            "{marker}{}{context}{unavailable}{thinking}{current}",
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
    if let Some(summary) = unavailable_summary(&entry.unavailable_reasons) {
        return format!("{}  {summary}", t!("dialog.model_picker_unavailable_label"));
    }
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

pub(crate) fn unavailable_summary(reasons: &[ProviderUnavailableReason]) -> Option<String> {
    if reasons.is_empty() {
        return None;
    }
    Some(
        reasons
            .iter()
            .map(unavailable_reason_label)
            .collect::<Vec<_>>()
            .join("; "),
    )
}

fn unavailable_reason_label(reason: &ProviderUnavailableReason) -> String {
    match reason {
        ProviderUnavailableReason::MissingBaseUrl => {
            t!("dialog.model_picker_unavailable_base_url").to_string()
        }
        ProviderUnavailableReason::MissingApiKey { env_key } => t!(
            "dialog.model_picker_unavailable_api_key",
            env_key = env_key.as_str()
        )
        .to_string(),
        ProviderUnavailableReason::NoModels => {
            t!("dialog.model_picker_unavailable_no_models").to_string()
        }
    }
}

/// User-facing role display name. Lookups go through i18n so the
/// translation table owns the wording — ASCII fallbacks aren't
/// hardcoded here.
fn role_display(role: ModelRole) -> String {
    let key = match role {
        ModelRole::Main => "role.main",
        ModelRole::Fast => "role.fast",
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
