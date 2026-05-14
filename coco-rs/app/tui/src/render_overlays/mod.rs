//! Overlay rendering — one file per category to stay under the 800-LoC
//! module-size guideline.
//!
//! Each submodule produces `(title, body, border_color)` for the caller to
//! wrap in a centered `Paragraph` with a `Block` border.

mod confirm;
mod context_viz;
mod diff;
mod help;
mod permission;
mod pickers;
mod question;
mod rewind;
mod search;
mod settings;
mod transcript;

use ratatui::prelude::*;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use crate::i18n::t;
use crate::presentation::layout;
use crate::state::AppState;
use crate::state::Overlay;
use crate::theme::Theme;

/// Render a modal overlay centered on screen.
pub(crate) fn render_overlay(
    frame: &mut Frame,
    area: Rect,
    overlay: &Overlay,
    state: &AppState,
    theme: &Theme,
) {
    if let Overlay::ModelPicker(m) = overlay {
        pickers::render_model_picker(frame, area, m, theme);
        return;
    }

    let (title, body, border_color) = overlay_content(overlay, state, theme);

    let width = ((area.width as u32 * 70 / 100) as u16).clamp(40, 100);
    let height = body
        .lines()
        .count()
        .saturating_add(4)
        .min(u16::MAX as usize) as u16;
    let overlay_area = layout::centered_fixed_area(area, width, height);

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
        Overlay::Permission(p) => permission::permission_content(p, theme),
        Overlay::Help => help::help_content(state, theme),
        Overlay::Error(msg) => (
            t!("dialog.title_error").to_string(),
            msg.clone(),
            theme.error,
        ),
        Overlay::PlanExit(p) => {
            confirm::plan_exit_content(p, state.session.bypass_permissions_available, theme)
        }
        Overlay::PlanEntry(p) => confirm::plan_entry_content(p, theme),
        Overlay::CostWarning(c) => confirm::cost_warning_content(c, theme),
        Overlay::ModelPicker(m) => pickers::model_picker_content(m, theme),
        Overlay::CommandPalette(cp) => pickers::command_palette_content(cp, theme),
        Overlay::SessionBrowser(s) => pickers::session_browser_content(s, theme),
        Overlay::Question(q) => question::question_content(q, theme),
        Overlay::Elicitation(e) => confirm::elicitation_content(e, theme),
        Overlay::SandboxPermission(s) => confirm::sandbox_content(s, theme),
        Overlay::GlobalSearch(g) => search::global_search_content(g, theme),
        Overlay::QuickOpen(q) => pickers::quick_open_content(q, theme),
        Overlay::Export(e) => pickers::export_content(e, theme),
        Overlay::DiffView(d) => diff::diff_view_content(d, theme),
        Overlay::McpServerApproval(m) => confirm::mcp_server_approval_content(m, theme),
        Overlay::WorktreeExit(w) => confirm::worktree_exit_content(w, theme),
        Overlay::Doctor(d) => confirm::doctor_content(d, theme),
        Overlay::Bridge(b) => confirm::bridge_content(b, theme),
        Overlay::InvalidConfig(ic) => confirm::invalid_config_content(ic, theme),
        Overlay::IdleReturn(ir) => confirm::idle_return_content(ir, theme),
        Overlay::Trust(tr) => confirm::trust_content(tr, theme),
        Overlay::AutoModeOptIn(a) => confirm::auto_mode_opt_in_content(a, theme),
        Overlay::BypassPermissions(bp) => confirm::bypass_permissions_content(bp, theme),
        Overlay::TaskDetail(td) => confirm::task_detail_content(td, theme),
        Overlay::Feedback(f) => confirm::feedback_content(f, theme),
        Overlay::McpServerSelect(ms) => pickers::mcp_server_select_content(ms, theme),
        Overlay::ContextVisualization => context_viz::context_viz_content(state, theme),
        Overlay::Rewind(r) => rewind::rewind_overlay_content(r, theme),
        Overlay::Settings(s) => settings::settings_overlay_content(s, theme),
        Overlay::PlanApproval(p) => confirm::plan_approval_content(p, theme),
        Overlay::MemoryDialog(m) => pickers::memory_dialog_content(m, theme),
        Overlay::Transcript(t) => transcript::transcript_overlay_content(state, t, theme),
    }
}
