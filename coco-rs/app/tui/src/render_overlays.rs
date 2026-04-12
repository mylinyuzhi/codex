//! Overlay rendering — extracted from render.rs to stay under 800 LoC.
//!
//! Each overlay variant produces `(title, body, border_color)` which the
//! caller wraps in a centered `Paragraph` with a `Block` border.

use ratatui::prelude::*;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use crate::state::AppState;
use crate::state::Overlay;
use crate::state::ui::PermissionDetail;
use crate::state::ui::RiskLevel;
use crate::theme::Theme;

/// Render a modal overlay centered on screen.
pub(crate) fn render_overlay(
    frame: &mut Frame,
    area: Rect,
    overlay: &Overlay,
    state: &AppState,
    theme: &Theme,
) {
    let (title, body, border_color) = overlay_content(overlay, state, theme);

    // Center the overlay
    let width = (area.width * 70 / 100).clamp(40, 100);
    let height = (body.lines().count() as u16 + 4).min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let overlay_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_area);

    let content = Paragraph::new(body).wrap(Wrap { trim: false }).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(border_color)),
    );

    frame.render_widget(content, overlay_area);
}

/// Produce (title, body, border_color) for every overlay variant.
fn overlay_content(overlay: &Overlay, state: &AppState, theme: &Theme) -> (String, String, Color) {
    match overlay {
        Overlay::Permission(p) => permission_content(p, theme),
        Overlay::Help => help_content(theme),
        Overlay::Error(msg) => (" Error ".to_string(), msg.clone(), theme.error),
        Overlay::PlanExit(p) => (
            " Exit Plan Mode ".to_string(),
            format!(
                "{}\n\n[Y] Confirm  [N] Cancel",
                p.plan_content.as_deref().unwrap_or("Exit plan mode?")
            ),
            theme.plan_mode,
        ),
        Overlay::PlanEntry(p) => (
            " Enter Plan Mode ".to_string(),
            format!("{}\n\n[Y] Confirm  [N] Cancel", p.description),
            theme.plan_mode,
        ),
        Overlay::CostWarning(c) => (
            " Cost Warning ".to_string(),
            format!(
                "Current cost: ${:.2}\nThreshold: ${:.2}\n\nContinue? [Y/N]",
                c.current_cost_cents as f64 / 100.0,
                c.threshold_cents as f64 / 100.0
            ),
            theme.warning,
        ),
        Overlay::ModelPicker(m) => model_picker_content(m, theme),
        Overlay::CommandPalette(cp) => command_palette_content(cp, theme),
        Overlay::SessionBrowser(s) => session_browser_content(s, theme),
        Overlay::Question(q) => question_content(q, theme),
        Overlay::Elicitation(e) => (
            format!(" {} ", e.server_name),
            format!("{}\n\n(Fill fields and press Enter)", e.message),
            theme.accent,
        ),
        Overlay::SandboxPermission(s) => (
            " Sandbox Permission ".to_string(),
            format!("{}\n\n[Y] Allow  [N] Deny", s.description),
            theme.error,
        ),
        Overlay::GlobalSearch(g) => global_search_content(g, theme),
        Overlay::QuickOpen(q) => quick_open_content(q, theme),
        Overlay::Export(e) => export_content(e, theme),
        Overlay::DiffView(d) => diff_view_content(d, theme),
        Overlay::McpServerApproval(m) => (
            " MCP Server ".to_string(),
            format!(
                "Server: {}\n{}\nTools: {}\n\n[Y] Approve  [N] Deny",
                m.server_name,
                m.server_url.as_deref().unwrap_or(""),
                m.tools.join(", ")
            ),
            theme.accent,
        ),
        Overlay::WorktreeExit(w) => {
            let files = if w.changed_files.is_empty() {
                "No uncommitted changes".to_string()
            } else {
                w.changed_files
                    .iter()
                    .map(|f| format!("  {f}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            (
                " Exit Worktree ".to_string(),
                format!("Branch: {}\n\n{files}\n\n[Y] Exit  [N] Stay", w.branch),
                theme.warning,
            )
        }
        Overlay::Doctor(d) => {
            let checks: Vec<String> = d
                .checks
                .iter()
                .map(|c| {
                    let icon = if c.passed { "✓" } else { "✗" };
                    format!("  {icon} {}: {}", c.name, c.message)
                })
                .collect();
            let body = if checks.is_empty() {
                "Running diagnostics...\n\nEsc Close".to_string()
            } else {
                format!("{}\n\nEsc Close", checks.join("\n"))
            };
            (" Doctor ".to_string(), body, theme.primary)
        }
        Overlay::Bridge(b) => (
            format!(" Bridge: {} ", b.bridge_type),
            format!("Status: {}\n\n{}\n\nEsc Close", b.status, b.details),
            theme.accent,
        ),
        Overlay::InvalidConfig(ic) => {
            let errors = ic
                .errors
                .iter()
                .map(|e| format!("  • {e}"))
                .collect::<Vec<_>>()
                .join("\n");
            (
                " Invalid Config ".to_string(),
                format!("Configuration errors:\n\n{errors}\n\nEsc Dismiss"),
                theme.error,
            )
        }
        Overlay::IdleReturn(ir) => {
            let mins = ir.idle_duration_secs / 60;
            (
                " Welcome Back ".to_string(),
                format!("You were away for {mins} minutes.\n\nEnter Continue"),
                theme.primary,
            )
        }
        Overlay::Trust(t) => (
            " Trust ".to_string(),
            format!(
                "Trust directory?\n\n  {}\n\n{}\n\n[Y] Trust  [N] Deny",
                t.path, t.description
            ),
            theme.warning,
        ),
        Overlay::AutoModeOptIn(a) => (
            " Auto Mode ".to_string(),
            format!("{}\n\nEnable auto-approve mode? [Y/N]", a.description),
            theme.primary,
        ),
        Overlay::BypassPermissions(bp) => (
            " Bypass Permissions ".to_string(),
            format!(
                "Current mode: {}\n\nSwitch to bypass mode?\n\n[Y] Enable  [N] Cancel",
                bp.current_mode
            ),
            theme.error,
        ),
        Overlay::TaskDetail(td) => {
            let output_lines: Vec<&str> = td.output.lines().collect();
            let visible: String = output_lines
                .iter()
                .skip(td.scroll as usize)
                .take(20)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            (
                format!(" Task: {} ", td.task_type),
                format!(
                    "{}\nStatus: {}\n\n{visible}\n\n↑/↓ Scroll  Esc Close",
                    td.description, td.status
                ),
                theme.primary,
            )
        }
        Overlay::Feedback(f) => {
            let items: Vec<String> = f
                .options
                .iter()
                .enumerate()
                .map(|(i, opt)| {
                    let marker = if i as i32 == f.selected { "▸ " } else { "  " };
                    format!("{marker}{opt}")
                })
                .collect();
            (
                " Feedback ".to_string(),
                format!("{}\n\n{}", f.prompt, items.join("\n")),
                theme.primary,
            )
        }
        Overlay::McpServerSelect(ms) => {
            let items: Vec<String> = ms
                .servers
                .iter()
                .map(|s| {
                    let check = if s.selected { "[x]" } else { "[ ]" };
                    format!("  {check} {} ({} tools)", s.name, s.tool_count)
                })
                .collect();
            (
                " Select MCP Servers ".to_string(),
                format!("Filter: {}\n\n{}", ms.filter, items.join("\n")),
                theme.accent,
            )
        }
        Overlay::ContextVisualization => context_viz_content(state, theme),
        Overlay::Rewind(r) => rewind_overlay_content(r, theme),
    }
}

fn permission_content(
    p: &crate::state::ui::PermissionOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let detail = match &p.detail {
        PermissionDetail::Bash {
            command,
            risk_description,
            working_dir,
        } => {
            let mut s = format!("Command:\n  $ {command}");
            if let Some(dir) = working_dir {
                s.push_str(&format!("\n\nDirectory: {dir}"));
            }
            if let Some(risk) = risk_description {
                s.push_str(&format!("\n\n⚠ {risk}"));
            }
            s
        }
        PermissionDetail::FileEdit { path, diff } => {
            let preview = if diff.len() > 500 {
                format!("{}...", &diff[..500])
            } else {
                diff.clone()
            };
            format!("File: {path}\n\n{preview}")
        }
        PermissionDetail::FileWrite {
            path,
            content_preview,
            is_new_file,
        } => {
            let action = if *is_new_file { "Create" } else { "Overwrite" };
            format!("{action}: {path}\n\n{content_preview}")
        }
        PermissionDetail::Filesystem { operation, path } => format!("{operation}: {path}"),
        PermissionDetail::WebFetch { url, method } => format!("{method} {url}"),
        PermissionDetail::Skill {
            skill_name,
            skill_description,
        } => {
            let desc = skill_description.as_deref().unwrap_or("");
            format!("Skill: /{skill_name}\n{desc}")
        }
        PermissionDetail::SedEdit {
            path,
            pattern,
            replacement,
        } => format!("File: {path}\nPattern: {pattern}\nReplace: {replacement}"),
        PermissionDetail::NotebookEdit {
            path,
            cell_id,
            change_preview,
        } => format!("Notebook: {path}\nCell: {cell_id}\n\n{change_preview}"),
        PermissionDetail::McpTool {
            server_name,
            tool_name,
            input_preview,
        } => format!("MCP: {server_name}::{tool_name}\n\n{input_preview}"),
        PermissionDetail::PowerShell {
            command,
            risk_description,
            working_dir,
        } => {
            let mut s = format!("PowerShell:\n  PS> {command}");
            if let Some(dir) = working_dir {
                s.push_str(&format!("\n\nDirectory: {dir}"));
            }
            if let Some(risk) = risk_description {
                s.push_str(&format!("\n\n⚠ {risk}"));
            }
            s
        }
        PermissionDetail::ComputerUse {
            action,
            description,
        } => format!("Computer Use: {action}\n\n{description}"),
        PermissionDetail::Generic { input_preview } => input_preview.clone(),
    };

    let risk_badge = match p.risk_level {
        Some(RiskLevel::Low) => " [LOW] ",
        Some(RiskLevel::Medium) => " [MEDIUM] ",
        Some(RiskLevel::High) => " [HIGH] ",
        None => "",
    };
    let title = if risk_badge.is_empty() {
        format!(" {} ", p.tool_name)
    } else {
        format!(" {}{risk_badge}", p.tool_name)
    };

    let classifier_line = if let Some(rule) = &p.classifier_auto_approved {
        format!("\n✓ Auto-approved · matched '{rule}'")
    } else if p.classifier_checking {
        "\n⟳ Checking...".to_string()
    } else {
        String::new()
    };

    let actions = if p.show_always_allow {
        "[Y] Approve  [N] Deny  [A] Always Allow"
    } else {
        "[Y] Approve  [N] Deny"
    };

    let border = match p.risk_level {
        Some(RiskLevel::High) => theme.error,
        Some(RiskLevel::Medium) => theme.warning,
        Some(RiskLevel::Low) | None => theme.warning,
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

fn help_content(theme: &Theme) -> (String, String, Color) {
    (
        " Help ".to_string(),
        [
            "Tab           Toggle plan mode",
            "Shift+Tab     Cycle permission mode",
            "Ctrl+T        Cycle thinking level",
            "Ctrl+M        Cycle model",
            "Ctrl+C        Interrupt",
            "Ctrl+L        Clear screen",
            "Ctrl+K        Kill to end of line",
            "Ctrl+Y        Yank killed text",
            "Ctrl+E        External editor",
            "Ctrl+P        Command palette",
            "Ctrl+S        Session browser",
            "Ctrl+Shift+F  Global search",
            "Ctrl+O        Quick open file",
            "Ctrl+W        Context window",
            "F6            Focus next panel",
            "Ctrl+Q        Quit",
            "?/F1          This help",
            "Esc           Close overlay",
            "PageUp/Down   Scroll",
        ]
        .join("\n"),
        theme.primary,
    )
}

fn model_picker_content(
    m: &crate::state::ui::ModelPickerOverlay,
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
        "Type to filter...".to_string()
    } else {
        format!("Filter: {}", m.filter)
    };

    (
        " Model ".to_string(),
        format!(
            "{filter_line}\n\n{}\n\n↑/↓ Navigate  Enter Select  Esc Cancel",
            items.join("\n")
        ),
        theme.primary,
    )
}

fn command_palette_content(
    cp: &crate::state::ui::CommandPaletteOverlay,
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
        "Type to filter commands...".to_string()
    } else {
        format!("Filter: {}", cp.filter)
    };

    (
        " Commands ".to_string(),
        format!(
            "{filter_line}\n\n{}\n\n↑/↓ Navigate  Enter Select  Esc Cancel",
            items.join("\n")
        ),
        theme.accent,
    )
}

fn session_browser_content(
    s: &crate::state::ui::SessionBrowserOverlay,
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
                "{marker}{} — {} msgs — {}",
                session.label, session.message_count, session.created_at
            )
        })
        .collect();

    let body = if items.is_empty() {
        "No saved sessions".to_string()
    } else {
        let filter_line = if s.filter.is_empty() {
            "Type to filter sessions...".to_string()
        } else {
            format!("Filter: {}", s.filter)
        };
        format!(
            "{filter_line}\n\n{}\n\n↑/↓ Navigate  Enter Resume  Esc Cancel",
            items.join("\n")
        )
    };

    (" Sessions ".to_string(), body, theme.primary)
}

fn question_content(
    q: &crate::state::ui::QuestionOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let items: Vec<String> = q
        .options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let marker = if i as i32 == q.selected { "▸ " } else { "  " };
            format!("{marker}{opt}")
        })
        .collect();
    (
        " Question ".to_string(),
        format!(
            "{}\n\n{}\n\n↑/↓ Navigate  Enter Select",
            q.question,
            items.join("\n")
        ),
        theme.primary,
    )
}

fn global_search_content(
    g: &crate::state::ui::GlobalSearchOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let query_line = if g.query.is_empty() {
        "Type to search...".to_string()
    } else {
        format!("Search: {}", g.query)
    };

    let results: Vec<String> = g
        .results
        .iter()
        .enumerate()
        .take(20)
        .map(|(i, r)| {
            let marker = if i as i32 == g.selected { "▸ " } else { "  " };
            format!("{marker}{}:{} {}", r.file, r.line_number, r.content.trim())
        })
        .collect();

    let status = if g.is_searching {
        "\n⟳ Searching..."
    } else if g.results.is_empty() && !g.query.is_empty() {
        "\nNo results"
    } else {
        ""
    };

    (
        " Global Search ".to_string(),
        format!(
            "{query_line}{status}\n\n{}\n\nEsc Cancel",
            results.join("\n")
        ),
        theme.primary,
    )
}

fn quick_open_content(
    q: &crate::state::ui::QuickOpenOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let filter_line = if q.filter.is_empty() {
        "Type file name...".to_string()
    } else {
        format!("Open: {}", q.filter)
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
        " Quick Open ".to_string(),
        format!(
            "{filter_line}\n\n{}\n\nEnter Open  Esc Cancel",
            items.join("\n")
        ),
        theme.primary,
    )
}

fn export_content(e: &crate::state::ui::ExportOverlay, theme: &Theme) -> (String, String, Color) {
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
        " Export Transcript ".to_string(),
        format!(
            "Select format:\n\n{}\n\n↑/↓ Navigate  Enter Export  Esc Cancel",
            items.join("\n")
        ),
        theme.primary,
    )
}

fn diff_view_content(
    d: &crate::state::ui::DiffViewOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    // Show file path header + colored diff lines with scroll offset
    let all_lines: Vec<&str> = d.diff.lines().collect();
    let total = all_lines.len();
    let offset = (d.scroll as usize).min(total);
    let visible: String = all_lines
        .iter()
        .skip(offset)
        .take(30)
        .map(|line| {
            if line.starts_with('+') && !line.starts_with("+++") {
                format!("  + {}", line.strip_prefix('+').unwrap_or(line))
            } else if line.starts_with('-') && !line.starts_with("---") {
                format!("  - {}", line.strip_prefix('-').unwrap_or(line))
            } else if line.starts_with("@@") {
                format!("  {line}")
            } else {
                format!("    {}", line.strip_prefix(' ').unwrap_or(line))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let position = if total > 0 {
        format!(" [{}/{total}]", offset + 1)
    } else {
        String::new()
    };
    (
        format!(" Diff: {}{position} ", d.path),
        format!("{visible}\n\n↑/↓ Scroll  Esc Close"),
        theme.primary,
    )
}

fn context_viz_content(state: &AppState, theme: &Theme) -> (String, String, Color) {
    let used = state.session.context_window_used;
    let total = state.session.context_window_total.max(1);
    let pct = (used * 100) / total;
    let bar_width = 40;
    let filled = (bar_width * pct / 100) as usize;
    let empty = bar_width as usize - filled;
    let bar = format!("[{}{}] {pct}%", "█".repeat(filled), "░".repeat(empty));

    let tokens = &state.session.token_usage;
    let body = format!(
        "{bar}\n\nInput:  {}\nOutput: {}\nCache:  {}\n\nUsed: {} / {}",
        crate::render::format_token_count(tokens.input_tokens),
        crate::render::format_token_count(tokens.output_tokens),
        crate::render::format_token_count(tokens.cache_read_tokens),
        crate::render::format_token_count(used as i64),
        crate::render::format_token_count(total as i64),
    );

    (
        " Context Window ".to_string(),
        format!("{body}\n\nEsc Close"),
        theme.primary,
    )
}

/// Render rewind overlay content.
///
/// TS: MessageSelector.tsx — two-phase UI:
/// 1. MessageSelect: list of user messages with selection marker
/// 2. RestoreOptions: restore type choices with diff preview
fn rewind_overlay_content(
    r: &crate::state::rewind::RewindOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    use crate::state::rewind::RewindPhase;
    use crate::update_rewind;

    match r.phase {
        RewindPhase::MessageSelect => {
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
                " Rewind ".to_string(),
                format!(
                    "Select a checkpoint to rewind to:\n\n{}{scroll_hint}\n\n\u{2191}/\u{2193} Navigate  Enter Select  Esc Cancel",
                    items.join("\n")
                ),
                theme.accent,
            )
        }
        RewindPhase::RestoreOptions => {
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
                    format!("{marker} {}", opt.label)
                })
                .collect();

            let diff_info = if let Some(ref stats) = r.diff_stats {
                format!(
                    "\n\n{} files changed, +{} -{} lines",
                    stats.files_changed, stats.insertions, stats.deletions
                )
            } else {
                String::new()
            };

            (
                " Rewind ".to_string(),
                format!(
                    "Rewind to {msg_label}:\n\n{}{diff_info}\n\n\u{2191}/\u{2193} Navigate  Enter Confirm  Esc Back",
                    items.join("\n")
                ),
                theme.accent,
            )
        }
        RewindPhase::Confirming => (
            " Rewind ".to_string(),
            "Rewinding...".to_string(),
            theme.accent,
        ),
    }
}
