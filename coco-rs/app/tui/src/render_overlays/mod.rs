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
use crate::presentation::styles::UiStyles;
use crate::state::AppState;
use crate::state::Overlay;

/// Render a modal overlay centered on screen.
pub(crate) fn render_overlay(
    frame: &mut Frame,
    area: Rect,
    overlay: &Overlay,
    state: &AppState,
    styles: UiStyles<'_>,
) {
    // CommandPalette renders inline above the input area as a borderless
    // suggestion list (handled by the active viewport renderer).
    // Mirrors TS `PromptInputFooterSuggestions` — no centered modal.
    if matches!(overlay, Overlay::CommandPalette(_)) {
        return;
    }

    if let Overlay::ModelPicker(m) = overlay {
        pickers::render_model_picker(frame, area, m, styles);
        return;
    }

    let (title, body, border_color) = overlay_content(overlay, state, styles);

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
pub(crate) fn overlay_content(
    overlay: &Overlay,
    state: &AppState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    match overlay {
        Overlay::Permission(p) => permission::permission_content(p, styles),
        Overlay::Help => help::help_content(state, styles),
        Overlay::Error(msg) => (
            t!("dialog.title_error").to_string(),
            msg.clone(),
            styles.error(),
        ),
        Overlay::PlanExit(p) => {
            confirm::plan_exit_content(p, state.session.bypass_permissions_available, styles)
        }
        Overlay::PlanEntry(p) => confirm::plan_entry_content(p, styles),
        Overlay::CostWarning(c) => confirm::cost_warning_content(c, styles),
        Overlay::ModelPicker(m) => pickers::model_picker_content(m, styles),
        Overlay::SessionBrowser(s) => pickers::session_browser_content(s, styles),
        Overlay::Question(q) => question::question_content(q, styles),
        Overlay::Elicitation(e) => confirm::elicitation_content(e, styles),
        Overlay::SandboxPermission(s) => confirm::sandbox_content(s, styles),
        Overlay::GlobalSearch(g) => search::global_search_content(g, styles),
        Overlay::QuickOpen(q) => pickers::quick_open_content(q, styles),
        Overlay::Export(e) => pickers::export_content(e, styles),
        Overlay::DiffView(d) => diff::diff_view_content(d, styles),
        Overlay::McpServerApproval(m) => confirm::mcp_server_approval_content(m, styles),
        Overlay::WorktreeExit(w) => confirm::worktree_exit_content(w, styles),
        Overlay::Doctor(d) => confirm::doctor_content(d, styles),
        Overlay::Bridge(b) => confirm::bridge_content(b, styles),
        Overlay::InvalidConfig(ic) => confirm::invalid_config_content(ic, styles),
        Overlay::IdleReturn(ir) => confirm::idle_return_content(ir, styles),
        Overlay::Trust(tr) => confirm::trust_content(tr, styles),
        Overlay::AutoModeOptIn(a) => confirm::auto_mode_opt_in_content(a, styles),
        Overlay::BypassPermissions(bp) => confirm::bypass_permissions_content(bp, styles),
        Overlay::TaskDetail(td) => confirm::task_detail_content(td, styles),
        Overlay::Feedback(f) => confirm::feedback_content(f, styles),
        Overlay::McpServerSelect(ms) => pickers::mcp_server_select_content(ms, styles),
        Overlay::ContextVisualization => context_viz::context_viz_content(state, styles),
        Overlay::Rewind(r) => rewind::rewind_overlay_content(r, styles),
        Overlay::Settings(s) => settings::settings_overlay_content(s, styles),
        Overlay::PlanApproval(p) => confirm::plan_approval_content(p, styles),
        Overlay::MemoryDialog(m) => pickers::memory_dialog_content(m, styles),
        Overlay::Transcript(t) => transcript::transcript_overlay_content(state, t, styles),
        // CommandPalette renders inline (handled by the early-return in
        // `render_overlay`). The match remains exhaustive but the arm is
        // unreachable.
        Overlay::CommandPalette(_) => unreachable!("CommandPalette renders inline above input"),
    }
}
