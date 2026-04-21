//! Overlay action handlers — approve/deny/filter/navigate/confirm.
//!
//! Factored out of `update.rs` to keep the top-level dispatch under 500 LoC.
//! All helpers are internal to the update module.

use coco_types::PermissionMode;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::constants;
use crate::state::AppState;
use crate::state::CommandOption;
use crate::state::CommandPaletteOverlay;
use crate::state::ExportFormat;
use crate::state::ModelOption;
use crate::state::ModelPickerOverlay;
use crate::state::Overlay;
use crate::state::SessionBrowserOverlay;
use crate::state::SessionOption;
use crate::state::SuggestionKind;
use crate::update_rewind;

/// Splice the currently selected suggestion back into the input buffer.
///
/// Replaces everything from `trigger_pos` to the cursor with the selection's
/// label (stripped of its trigger-equivalent prefix). Adds a trailing space
/// so the user can continue typing. Clears `active_suggestions` on commit so
/// the popup dismisses — the user is out of the trigger range now.
fn accept_suggestion(state: &mut AppState) {
    let Some(sug) = state.ui.active_suggestions.take() else {
        return;
    };
    let Some(item) = sug.items.get(sug.selected as usize).cloned() else {
        state.ui.active_suggestions = None;
        return;
    };
    // Slash labels are already prefixed with `/`; mention labels are not.
    // For @-mentions we re-add the `@` to stay consistent with the text
    // the user originally typed.
    let insertion = match sug.kind {
        SuggestionKind::SlashCommand => format!("{} ", item.label),
        SuggestionKind::File | SuggestionKind::Agent | SuggestionKind::Symbol => {
            let prefix = match sug.kind {
                SuggestionKind::Agent => "@agent-",
                SuggestionKind::Symbol => "@#",
                _ => "@",
            };
            format!("{prefix}{} ", item.label)
        }
    };

    let text = &state.ui.input.text;
    let chars: Vec<char> = text.chars().collect();
    let start = (sug.trigger_pos as usize).min(chars.len());
    let end = (state.ui.input.cursor as usize).min(chars.len());
    let mut new_text: String = chars[..start].iter().collect();
    new_text.push_str(&insertion);
    new_text.push_str(&chars[end..].iter().collect::<String>());
    state.ui.input.text = new_text;
    state.ui.input.cursor = (start + insertion.chars().count()) as i32;
}

/// Handle `Approve` for the current overlay.
pub(super) async fn approve(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    match &state.ui.overlay {
        Some(Overlay::Permission(p)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: p.request_id.clone(),
                    approved: true,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::SandboxPermission(s)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: s.request_id.clone(),
                    approved: true,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::McpServerApproval(m)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: m.request_id.clone(),
                    approved: true,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::PlanEntry(_)) => {
            // Entry: flip into Plan.
            state.toggle_plan_mode();
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: state.session.permission_mode,
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::PlanExit(p)) => {
            // Exit: target mode depends on which approval option the
            // user picked. `RestorePrePlan` defers the mode switch to
            // `ExitPlanModeTool::execute`, which writes the restored
            // mode onto `app_state.permission_mode` (source of truth);
            // the other variants explicitly set the target mode via
            // `SetPermissionMode` because the user's pick overrides
            // the stashed `pre_plan_mode`.
            //
            // Defense in depth: if the overlay somehow holds
            // `BypassPermissions` but the capability gate is off,
            // down-shift to `AcceptEdits` rather than silently
            // escalating. Normal paths can't reach this (the renderer
            // and cycle honor the gate) but a stale overlay is cheap
            // to defend against.
            let mut next = p.next_mode;
            if next == crate::state::PlanExitTarget::BypassPermissions
                && !state.session.bypass_permissions_available
            {
                next = crate::state::PlanExitTarget::AcceptEdits;
            }
            let target = next.resolve().unwrap_or(PermissionMode::Default);
            state.session.permission_mode = target;
            let _ = command_tx
                .send(UserCommand::SetPermissionMode { mode: target })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(
            Overlay::Trust(_)
            | Overlay::AutoModeOptIn(_)
            | Overlay::BypassPermissions(_)
            | Overlay::WorktreeExit(_),
        ) => {
            state.ui.dismiss_overlay();
        }
        _ => {
            state.ui.dismiss_overlay();
        }
    }
}

/// Handle `Deny` for the current overlay.
pub(super) async fn deny(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    match &state.ui.overlay {
        Some(Overlay::Permission(p)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: p.request_id.clone(),
                    approved: false,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::SandboxPermission(s)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: s.request_id.clone(),
                    approved: false,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::McpServerApproval(m)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: m.request_id.clone(),
                    approved: false,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::PlanExit(p)) => {
            // User rejected the plan. Surface a visible record in the
            // chat transcript — TS parity: `RejectedPlanMessage`
            // component renders the plan in a bordered block. Mode
            // stays in `Plan` (no mutation); the user can keep
            // refining or exit via the normal toggle.
            let plan = p.plan_content.clone().unwrap_or_default();
            // Monotonic-ish id; TUI has no uuid dep and plan rejections
            // are rare enough that nanos collisions are moot.
            let id = format!(
                "plan-rejected-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default()
            );
            let body = if plan.trim().is_empty() {
                crate::i18n::t!("plan.rejected_empty").to_string()
            } else {
                format!("{}\n\n{plan}", crate::i18n::t!("plan.rejected_header"),)
            };
            state
                .session
                .add_message(crate::state::session::ChatMessage::system_text(id, body));
            state.ui.dismiss_overlay();
        }
        _ => {
            state.ui.dismiss_overlay();
        }
    }
}

/// Handle `ApproveAll` (always-allow) for permission overlays.
pub(super) async fn approve_all(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    if let Some(Overlay::Permission(ref p)) = state.ui.overlay
        && p.show_always_allow
    {
        let _ = command_tx
            .send(UserCommand::ApprovalResponse {
                request_id: p.request_id.clone(),
                approved: true,
                always_allow: true,
                feedback: None,
                updated_input: None,
                permission_updates: vec![],
            })
            .await;
        state.ui.dismiss_overlay();
    }
}

/// Handle `ClassifierAutoApprove` — background classifier approved the pending
/// request before the user responded.
pub(super) async fn classifier_auto_approve(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
    request_id: String,
) {
    if let Some(Overlay::Permission(ref p)) = state.ui.overlay
        && p.request_id == request_id
    {
        let _ = command_tx
            .send(UserCommand::ApprovalResponse {
                request_id: p.request_id.clone(),
                approved: true,
                always_allow: false,
                feedback: None,
                updated_input: None,
                permission_updates: vec![],
            })
            .await;
        state.ui.dismiss_overlay();
    }
}

/// Push `c` into the current filterable overlay's filter string.
pub(super) fn filter(state: &mut AppState, c: char) {
    match &mut state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => {
            m.filter.push(c);
            m.selected = 0;
        }
        Some(Overlay::CommandPalette(cp)) => {
            cp.filter.push(c);
            cp.selected = 0;
        }
        Some(Overlay::SessionBrowser(s)) => {
            s.filter.push(c);
            s.selected = 0;
        }
        Some(Overlay::GlobalSearch(g)) => {
            g.query.push(c);
            g.selected = 0;
        }
        Some(Overlay::QuickOpen(q)) => {
            q.filter.push(c);
            q.selected = 0;
        }
        _ => {}
    }
}

/// Pop the last char from the current filterable overlay's filter string.
pub(super) fn filter_backspace(state: &mut AppState) {
    match &mut state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => {
            m.filter.pop();
            m.selected = 0;
        }
        Some(Overlay::CommandPalette(cp)) => {
            cp.filter.pop();
            cp.selected = 0;
        }
        Some(Overlay::SessionBrowser(s)) => {
            s.filter.pop();
            s.selected = 0;
        }
        Some(Overlay::GlobalSearch(g)) => {
            g.query.pop();
            g.selected = 0;
        }
        Some(Overlay::QuickOpen(q)) => {
            q.filter.pop();
            q.selected = 0;
        }
        _ => {}
    }
}

/// Move selection by `delta` in the current list/scrollable overlay.
pub(super) fn nav(state: &mut AppState, delta: i32) {
    // Autocomplete takes precedence over (non-existent) overlay.
    if state.ui.overlay.is_none()
        && let Some(ref mut sug) = state.ui.active_suggestions
    {
        let count = sug.items.len() as i32;
        sug.selected = (sug.selected + delta).clamp(0, (count - 1).max(0));
        return;
    }
    match &mut state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => {
            let count = filtered_models(m).len() as i32;
            m.selected = (m.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::CommandPalette(cp)) => {
            let count = filtered_commands(cp).len() as i32;
            cp.selected = (cp.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::SessionBrowser(s)) => {
            let count = filtered_sessions(s).len() as i32;
            s.selected = (s.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::GlobalSearch(g)) => {
            let count = g.results.len() as i32;
            g.selected = (g.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::QuickOpen(q)) => {
            let count = q.files.len() as i32;
            q.selected = (q.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::Export(e)) => {
            let count = e.formats.len() as i32;
            e.selected = (e.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::Question(q)) => {
            let count = q.options.len() as i32;
            q.selected = (q.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::Feedback(f)) => {
            let count = f.options.len() as i32;
            f.selected = (f.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::PlanExit(p)) => {
            // Cycle the exit target on Up/Down (or Tab — keybind layer
            // maps Tab to OverlayNext here). Bypass is conditional on
            // the capability gate; when gated off we stay on the
            // Restore/AcceptEdits two-way cycle. TS parity:
            // `buildPlanApprovalOptions` with
            // `isBypassPermissionsModeAvailable`.
            let order =
                crate::state::PlanExitTarget::available(state.session.bypass_permissions_available);
            let current_idx = order.iter().position(|t| *t == p.next_mode).unwrap_or(0) as i32;
            let len = order.len() as i32;
            let new_idx = ((current_idx + delta).rem_euclid(len)) as usize;
            p.next_mode = order[new_idx];
        }
        Some(Overlay::Rewind(r)) => {
            update_rewind::handle_rewind_nav(r, delta);
        }
        Some(Overlay::DiffView(d)) => {
            d.scroll = (d.scroll + delta * constants::SCROLL_LINE_STEP).max(0);
        }
        Some(Overlay::TaskDetail(t)) => {
            t.scroll = (t.scroll + delta * constants::SCROLL_LINE_STEP).max(0);
        }
        Some(Overlay::PlanApproval(p)) => {
            // Left/right (nav delta) toggles between Approve and Deny.
            if delta != 0 {
                p.toggle_focus();
            }
        }
        Some(Overlay::Settings(s)) => {
            let count = settings_item_count(s) as i32;
            s.selected = (s.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(
            Overlay::Help
            | Overlay::Doctor(_)
            | Overlay::ContextVisualization
            | Overlay::Bridge(_)
            | Overlay::InvalidConfig(_),
        ) => {
            state.ui.help_scroll =
                (state.ui.help_scroll + delta * constants::SCROLL_LINE_STEP).max(0);
        }
        _ => {}
    }
}

/// Confirm the currently selected item in a list overlay.
pub(super) async fn confirm(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    // Autocomplete popup takes precedence over (non-existent) overlay when
    // suggestions are active — pressing Tab/Enter accepts the selection.
    if state.ui.overlay.is_none() && state.ui.active_suggestions.is_some() {
        accept_suggestion(state);
        return;
    }
    let overlay = state.ui.overlay.take();
    match overlay {
        Some(Overlay::ModelPicker(m)) => {
            if let Some(model) = filtered_models(&m).get(m.selected as usize) {
                let _ = command_tx
                    .send(UserCommand::SetModel {
                        model: model.id.clone(),
                    })
                    .await;
                state.session.model = model.id.clone();
            }
        }
        Some(Overlay::CommandPalette(cp)) => {
            if let Some(cmd) = filtered_commands(&cp).get(cp.selected as usize) {
                // Intercept /rewind and /checkpoint to open overlay instead
                if cmd.name == "rewind" || cmd.name == "checkpoint" {
                    let overlay = update_rewind::build_rewind_overlay(state);
                    state.ui.overlay = Some(Overlay::Rewind(overlay));
                    return;
                }
                // /copy dispatches straight to the clipboard handler — no
                // round-trip through the agent. Mirrors codex-rs's
                // SlashCommand::Copy path.
                if cmd.name == "copy" {
                    super::clipboard::copy_last_message(state);
                    return;
                }
                let _ = command_tx
                    .send(UserCommand::ExecuteSkill {
                        name: cmd.name.clone(),
                        args: None,
                    })
                    .await;
            }
        }
        Some(Overlay::Rewind(mut r)) => {
            if let Some((message_id, restore_type)) = update_rewind::handle_rewind_confirm(&mut r) {
                let _ = command_tx
                    .send(UserCommand::Rewind {
                        message_id,
                        restore_type,
                    })
                    .await;
            } else {
                // Phase transition; put overlay back.
                state.ui.overlay = Some(Overlay::Rewind(r));
                return;
            }
        }
        Some(Overlay::SessionBrowser(s)) => {
            if let Some(session) = filtered_sessions(&s).get(s.selected as usize) {
                let _ = command_tx
                    .send(UserCommand::SubmitInput {
                        content: format!("/resume {}", session.id),
                        display_text: None,
                        images: Vec::new(),
                    })
                    .await;
            }
        }
        Some(Overlay::Export(e)) => {
            if let Some(fmt) = e.formats.get(e.selected as usize) {
                let cmd = match fmt {
                    ExportFormat::Markdown => "/export markdown",
                    ExportFormat::Json => "/export json",
                    ExportFormat::Text => "/export text",
                };
                let _ = command_tx
                    .send(UserCommand::SubmitInput {
                        content: cmd.to_string(),
                        display_text: None,
                        images: Vec::new(),
                    })
                    .await;
            }
        }
        Some(Overlay::Question(q)) => {
            if let Some(option) = q.options.get(q.selected as usize) {
                let _ = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id: q.request_id.clone(),
                        approved: true,
                        always_allow: false,
                        feedback: Some(option.clone()),
                        updated_input: None,
                        permission_updates: vec![],
                    })
                    .await;
            }
        }
        Some(Overlay::Settings(s)) => {
            // Only the Theme tab applies changes on Enter. Other tabs are
            // read-only (About) or populated asynchronously (OutputStyle,
            // Permissions). On Theme tab, swap the live theme immediately.
            if let crate::widgets::settings_panel::SettingsTab::Theme = s.active_tab
                && let Some(theme_name) = s.themes.get(s.selected as usize)
            {
                state.ui.theme = crate::theme::Theme::from_name(*theme_name);
            }
            // Keep settings open after selection — user may want to try
            // themes successively.
            state.ui.overlay = Some(Overlay::Settings(s));
            return;
        }
        // Plan-approval (team-lead side): Enter sends the response
        // keyed to the currently-focused button (Approve / Deny). The
        // engine translates this into a mailbox envelope back to the
        // teammate. TS parity: `ExitPlanModeV2Tool.ts:137-141` request
        // flow, leader-end resolution.
        Some(Overlay::PlanApproval(p)) => {
            let _ = command_tx
                .send(UserCommand::PlanApprovalResponse {
                    request_id: p.request_id.clone(),
                    teammate_agent: p.from.clone(),
                    approved: p.is_approve_focused(),
                    feedback: None,
                })
                .await;
            state.ui.dismiss_overlay();
            return;
        }
        // PlanExit confirm delegates to the approval handler so Enter
        // and Y take the same path (including the `next_mode` target).
        Some(Overlay::PlanExit(p)) => {
            let target = p.next_mode.resolve().unwrap_or(PermissionMode::Default);
            state.session.permission_mode = target;
            let _ = command_tx
                .send(UserCommand::SetPermissionMode { mode: target })
                .await;
            state.ui.dismiss_overlay();
            return;
        }
        // All remaining overlays: confirm = dismiss
        Some(
            Overlay::Permission(_)
            | Overlay::Help
            | Overlay::Error(_)
            | Overlay::PlanEntry(_)
            | Overlay::CostWarning(_)
            | Overlay::Elicitation(_)
            | Overlay::SandboxPermission(_)
            | Overlay::GlobalSearch(_)
            | Overlay::QuickOpen(_)
            | Overlay::DiffView(_)
            | Overlay::McpServerApproval(_)
            | Overlay::WorktreeExit(_)
            | Overlay::Doctor(_)
            | Overlay::Bridge(_)
            | Overlay::InvalidConfig(_)
            | Overlay::IdleReturn(_)
            | Overlay::Trust(_)
            | Overlay::AutoModeOptIn(_)
            | Overlay::BypassPermissions(_)
            | Overlay::TaskDetail(_)
            | Overlay::Feedback(_)
            | Overlay::McpServerSelect(_)
            | Overlay::ContextVisualization,
        ) => {
            // Dismiss on confirm (Enter/Esc)
        }
        None => {}
    }
    // Next queued overlay
    state.ui.overlay = state.ui.overlay_queue.pop_front();
}

/// Send `RequestDiffStats` for the selected message when a Rewind overlay
/// is active — TS: MessageSelector useEffect recomputes on index change.
pub(super) async fn request_diff_stats_if_rewind(
    state: &AppState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    if let Some(Overlay::Rewind(ref r)) = state.ui.overlay
        && let Some(msg) = r.messages.get(r.selected as usize)
    {
        let _ = command_tx
            .send(UserCommand::RequestDiffStats {
                message_id: msg.message_id.clone(),
            })
            .await;
    }
}

/// Rewind Esc: go back a phase before dismissing. Returns `true` if overlay
/// should be dismissed, `false` if a phase transition happened.
pub(super) fn rewind_cancel(state: &mut AppState) -> bool {
    if let Some(Overlay::Rewind(ref mut r)) = state.ui.overlay
        && !update_rewind::handle_rewind_cancel(r)
    {
        return false;
    }
    true
}

// ── filter helpers ──

fn filtered_models(m: &ModelPickerOverlay) -> Vec<&ModelOption> {
    let filter_lower = m.filter.to_lowercase();
    m.models
        .iter()
        .filter(|model| {
            filter_lower.is_empty() || model.label.to_lowercase().contains(&filter_lower)
        })
        .collect()
}

fn filtered_commands(cp: &CommandPaletteOverlay) -> Vec<&CommandOption> {
    let filter_lower = cp.filter.to_lowercase();
    cp.commands
        .iter()
        .filter(|cmd| filter_lower.is_empty() || cmd.name.to_lowercase().contains(&filter_lower))
        .collect()
}

fn filtered_sessions(s: &SessionBrowserOverlay) -> Vec<&SessionOption> {
    let filter_lower = s.filter.to_lowercase();
    s.sessions
        .iter()
        .filter(|sess| filter_lower.is_empty() || sess.label.to_lowercase().contains(&filter_lower))
        .collect()
}

/// Number of selectable items on the Settings active tab.
fn settings_item_count(s: &crate::widgets::settings_panel::SettingsPanelState) -> usize {
    use crate::widgets::settings_panel::SettingsTab;
    match s.active_tab {
        SettingsTab::Theme => s.themes.len(),
        SettingsTab::OutputStyle => s.output_styles.len(),
        SettingsTab::Permissions => s.permission_rules.len(),
        SettingsTab::About => 0,
    }
}
