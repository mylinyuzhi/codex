//! Overlay action handlers — approve/deny/filter/navigate/confirm.
//!
//! Factored out of `update.rs` to keep the top-level dispatch under 500 LoC.
//! All helpers are internal to the update module.

use coco_types::PermissionMode;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::constants;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::CommandOption;
use crate::state::CommandPaletteOverlay;
use crate::state::ExportFormat;
use crate::state::MemoryDialogEntry;
use crate::state::ModelEntry;
use crate::state::ModelPickerOverlay;
use crate::state::Overlay;
use crate::state::ProviderUnavailableReason;
use crate::state::SessionBrowserOverlay;
use crate::state::SessionOption;
use crate::state::SuggestionKind;
use crate::state::ui::Toast;
use crate::update_rewind;
use crate::widgets::suggestion_popup::SuggestionMeta;

/// Splice the currently selected suggestion back into the input buffer.
///
/// Replaces everything from `trigger_pos` to the cursor with a formatted
/// rendering of the selection. Mirrors TS `formatReplacementValue` +
/// `applyDirectorySuggestion` (`useTypeahead.tsx:148,237`):
///   - directories → `@<path>/` and **leave the popup open** so the user
///     can keep narrowing
///   - files       → `@<path> ` (trailing space, popup dismisses)
///   - paths with whitespace → auto-quoted: `@"<path>" `
///   - agents      → `@agent-<type> `
///   - symbols     → `@#<sym> `
///   - slash       → `<label> ` (label already has `/` prefix)
fn accept_suggestion(state: &mut AppState) {
    let Some(sug) = state.ui.active_suggestions.take() else {
        return;
    };
    let Some(item) = sug.items.get(sug.selected).cloned() else {
        state.ui.active_suggestions = None;
        return;
    };

    let is_directory = matches!(
        item.metadata,
        Some(SuggestionMeta::Path { is_directory: true })
    );

    let (insertion, keep_popup) = match sug.kind {
        SuggestionKind::SlashCommand => (format!("{} ", item.label), false),
        SuggestionKind::File => {
            let body = if item.label.contains(char::is_whitespace) {
                format!("@\"{}\"", item.label)
            } else {
                format!("@{}", item.label)
            };
            if is_directory {
                // Append `/` and re-detect the trigger so the popup
                // keeps showing entries inside the chosen directory.
                (format!("{body}/"), true)
            } else {
                (format!("{body} "), false)
            }
        }
        SuggestionKind::Agent => (format!("@agent-{} ", item.label), false),
        SuggestionKind::Symbol => (format!("@#{} ", item.label), false),
    };

    // Byte-offset splice via TextArea so multi-byte text before the
    // trigger doesn't shift the insertion point. Mirrors the TS pattern
    // where trigger pos and cursor are both UTF-16 code-unit offsets and
    // `text.slice(start, cursor).replace(...)` does the splice directly.
    let text_len = state.ui.input.text().len();
    let start = sug.trigger_pos.min(text_len);
    let end = state.ui.input.textarea.cursor().min(text_len);
    state
        .ui
        .input
        .textarea
        .replace_range(start..end, &insertion);
    state.ui.input.textarea.set_cursor(start + insertion.len());

    // For directory completions, re-trigger the popup so the next
    // search runs against the new prefix. `accept_suggestion` already
    // took `active_suggestions`; let `refresh_suggestions` install a
    // fresh popup keyed off the new query.
    if keep_popup {
        crate::autocomplete::refresh_suggestions(state);
    }
}

/// Handle `Approve` for the current overlay.
pub(super) async fn approve(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    match &state.ui.overlay {
        Some(Overlay::Permission(p)) => {
            // Multi-choice mode: 'y' commits the currently-focused
            // choice (Enter takes the same path via confirm()). The
            // chosen `value` is spliced into `updated_input` so the
            // tool's execute() can branch on it. A choice whose value
            // is "no" denies; everything else approves. Classic yes/no
            // mode (`choices.is_none()`) keeps the unconditional
            // `approved: true` path.
            let (approved, updated_input) = if p.choices.is_some() {
                let chosen_is_no = p
                    .choices
                    .as_ref()
                    .and_then(|cs| cs.get(p.selected_choice))
                    .map(|c| c.value.as_str())
                    == Some("no");
                (!chosen_is_no, build_choice_payload(p))
            } else {
                (true, None)
            };
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: p.request_id.clone(),
                    approved,
                    always_allow: false,
                    feedback: None,
                    updated_input,
                    permission_updates: vec![],
                    content_blocks: None,
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
                    content_blocks: None,
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
                    content_blocks: None,
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
        Some(Overlay::BypassPermissions(_)) => {
            // Defense in depth: even after the user clicks Approve we
            // re-check the capability gate before flipping. The cycle
            // path already filters Bypass when the gate is off, but a
            // stale overlay (e.g. opened earlier in the session, gate
            // toggled since) shouldn't be able to escalate. Drops
            // through to a no-op + neutral toast so the user knows the
            // click was acknowledged without surprise.
            if !state.session.bypass_permissions_available {
                state.ui.add_toast(crate::state::ui::Toast::warning(
                    t!("toast.bypass_unavailable").to_string(),
                ));
                state.ui.dismiss_overlay();
                return;
            }
            state.session.permission_mode = PermissionMode::BypassPermissions;
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: PermissionMode::BypassPermissions,
                })
                .await;
            state.ui.add_toast(crate::state::ui::Toast::warning(
                t!("toast.bypass_enabled").to_string(),
            ));
            state.ui.dismiss_overlay();
        }
        Some(Overlay::AutoModeOptIn(_)) => {
            state.session.permission_mode = PermissionMode::Auto;
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: PermissionMode::Auto,
                })
                .await;
            state.ui.add_toast(crate::state::ui::Toast::info(
                t!("toast.auto_mode_enabled").to_string(),
            ));
            state.ui.dismiss_overlay();
        }
        Some(Overlay::Trust(_) | Overlay::WorktreeExit(_)) => {
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
                    content_blocks: None,
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
                    content_blocks: None,
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
                    content_blocks: None,
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
        Some(Overlay::BypassPermissions(_)) | Some(Overlay::AutoModeOptIn(_)) => {
            // Declining bypass / auto opt-in keeps the current mode.
            // A toast confirms the cancel so the user doesn't doubt
            // whether the Shift+Tab landed silently.
            let current = crate::update::permission_mode_label(state.session.permission_mode);
            state.ui.add_toast(crate::state::ui::Toast::info(
                crate::i18n::t!("toast.permission_mode_unchanged", mode = current.as_str())
                    .to_string(),
            ));
            state.ui.dismiss_overlay();
        }
        _ => {
            state.ui.dismiss_overlay();
        }
    }
}

/// Handle `ApproveAll` (always-allow) for permission overlays.
///
/// Phase A: builds a Session-scoped allow rule for the tool. `tui_runner`
/// consumes the update via `coco_permissions::apply_permission_updates`
/// (live engine_config mutation) so subsequent same-tool calls in the
/// session don't re-prompt. Matches the rule shape produced by the
/// `/permissions allow <tool>` slash command in
/// `tui_runner::dispatch_permissions_mutation`, so both UX paths land in
/// the same place.
///
/// Phase B (out of scope): a destination sub-picker on the dialog will
/// let the user pick User / Project / Local; the runner already calls
/// `SettingsPermissionStore::persist_update` for those destinations.
pub(super) async fn approve_all(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    if let Some(Overlay::Permission(ref p)) = state.ui.overlay
        && p.show_always_allow
    {
        let update = coco_types::PermissionUpdate::AddRules {
            rules: vec![coco_types::PermissionRule {
                source: coco_types::PermissionRuleSource::Session,
                behavior: coco_types::PermissionBehavior::Allow,
                value: coco_types::PermissionRuleValue {
                    tool_pattern: p.tool_name.clone(),
                    rule_content: None,
                },
            }],
            destination: coco_types::PermissionUpdateDestination::Session,
        };

        let _ = command_tx
            .send(UserCommand::ApprovalResponse {
                request_id: p.request_id.clone(),
                approved: true,
                always_allow: true,
                feedback: None,
                updated_input: None,
                permission_updates: vec![update],
                content_blocks: None,
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
                content_blocks: None,
            })
            .await;
        state.ui.dismiss_overlay();
    }
}

/// Push `c` into the current filterable overlay's filter string.
pub(super) fn filter(state: &mut AppState, c: char) {
    // Question overlay specializes the keystroke routing: Space toggles
    // multi-select; printable chars edit the "Other" notes textarea
    // when that option is focused. Both consume the keystroke before
    // any filter logic. TS: `QuestionView.tsx` `onKeyDown` priority.
    if matches!(state.ui.overlay, Some(Overlay::Question(_))) {
        if c == ' ' {
            question_toggle_checked(state);
            return;
        }
        if question_notes_input(state, c) {
            return;
        }
        return; // Question overlay has no filter — silently swallow.
    }
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
    // Question overlay: when "Other" is focused, Backspace edits the
    // notes textarea. Otherwise no-op (Question has no filter).
    if matches!(state.ui.overlay, Some(Overlay::Question(_))) {
        question_notes_backspace(state);
        return;
    }
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
        if sug.items.is_empty() {
            sug.selected = 0;
        } else {
            let new = sug.selected as i32 + delta;
            sug.selected = new.clamp(0, sug.items.len() as i32 - 1) as usize;
        }
        return;
    }
    match &mut state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => {
            let count = filtered_models(m).len() as i32;
            m.selected = (m.selected + delta).clamp(0, (count - 1).max(0));
            // Re-derive effort from the newly-focused model's default
            // so the footer reflects "the model's preferred level"
            // unless the user has explicitly cycled past it.
            m.effort = filtered_models(m)
                .get(m.selected as usize)
                .and_then(|e| e.default_effort);
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
            // Up/Down moves the focused option within the *focused* question.
            // No-op when focus is on a footer item (Chat-about-this /
            // Skip-interview) — Tab/Shift+Tab cycle the focus between
            // questions and footer items, handled by `OverlayTabsNext`.
            if let crate::state::QuestionFocus::Question(idx) = q.focus
                && let Some(qi) = q.questions.get_mut(idx as usize)
            {
                let count = qi.options.len() as i32;
                let next = (qi.selected + delta).clamp(0, (count - 1).max(0));
                qi.selected = next;
                // TS `QuestionView.tsx:85-87`: focusing the `__other__`
                // option flips into text-input mode. Drop out when
                // moving away.
                qi.editing_notes = qi
                    .options
                    .get(next as usize)
                    .map(|o| o.label == crate::state::OTHER_OPTION_LABEL)
                    .unwrap_or(false);
            }
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
        Some(Overlay::Permission(p)) => {
            // Multi-choice mode (ExitPlanMode keep/clear/cancel etc.):
            // Up/Down moves the selected choice with saturation. In
            // classic yes/no mode this arm is a no-op — Approve / Deny
            // map to dedicated keystrokes ('y' / 'n'), not the cursor.
            if let Some(choices) = &p.choices
                && !choices.is_empty()
            {
                let count = choices.len() as i32;
                let current = p.selected_choice as i32;
                let next = (current + delta).clamp(0, count - 1);
                p.selected_choice = next as usize;
            }
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
        Some(Overlay::MemoryDialog(m)) => {
            let count = m.entries.len() as i32;
            m.selected = (m.selected + delta).clamp(0, (count - 1).max(0));
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
            if let Some(entry) = filtered_models(&m).get(m.selected as usize).copied() {
                if let Some(summary) = unavailable_summary(&entry.unavailable_reasons) {
                    state.ui.overlay = Some(Overlay::ModelPicker(m));
                    state.ui.add_toast(Toast::warning(format!(
                        "{} {summary}",
                        t!("dialog.model_picker_unavailable_label")
                    )));
                    return;
                }
                let _ = command_tx
                    .send(UserCommand::SetModelRole {
                        role: m.role,
                        provider: entry.provider.clone(),
                        model_id: entry.model_id.clone(),
                        effort: m.effort,
                    })
                    .await;
                // Optimistic local update for Main — non-Main roles
                // have no live mirror in `SessionState`, so the engine
                // is the source of truth there.
                if matches!(m.role, coco_types::ModelRole::Main) {
                    state.session.model = entry.model_id.clone();
                }
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
            // Capture the 1-based turn number for the protocol-level
            // `rewind/completed` notification before handle_rewind_confirm
            // mutates the overlay phase. `selected` is 0-based against the
            // filtered messages vec, so +1 yields the user-visible label.
            let rewound_turn = r.selected + 1;
            match update_rewind::handle_rewind_confirm(&mut r) {
                update_rewind::ConfirmOutcome::Dispatch {
                    message_id,
                    restore,
                } => {
                    // Keep the overlay open in `Confirming` phase while
                    // the rewind/summarize is in flight. TS:
                    // `MessageSelector.tsx:341-344`. `on_rewind_completed`
                    // dismisses the overlay when the engine notifies completion.
                    r.phase = crate::state::rewind::RewindPhase::Confirming;
                    state.ui.overlay = Some(Overlay::Rewind(r));
                    let _ = command_tx
                        .send(UserCommand::Rewind {
                            message_id,
                            restore_type: restore,
                            rewound_turn,
                        })
                        .await;
                }
                update_rewind::ConfirmOutcome::Phase => {
                    // Phase transition without dispatch — put overlay back.
                    state.ui.overlay = Some(Overlay::Rewind(r));
                }
                update_rewind::ConfirmOutcome::Dismiss => {
                    // Synthetic `(current)` row or preselected-Nevermind:
                    // close overlay (TS `MessageSelector.tsx:165` /
                    // line 186).
                    // (overlay already taken; do not put back.)
                }
            }
            return;
        }
        Some(Overlay::SessionBrowser(s)) => {
            if let Some(session) = filtered_sessions(&s).get(s.selected as usize) {
                let _ = command_tx
                    .send(UserCommand::SubmitInput {
                        user_message_id: uuid::Uuid::new_v4().to_string(),
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
                        user_message_id: uuid::Uuid::new_v4().to_string(),
                        content: cmd.to_string(),
                        display_text: None,
                        images: Vec::new(),
                    })
                    .await;
            }
        }
        Some(Overlay::Question(q)) => {
            use crate::state::QuestionFocus;
            match q.focus {
                QuestionFocus::Question(idx) => {
                    // Intermediate question → advance to the next.
                    // Last question → submit all answers via the
                    // updated_input splice. TS `nextQuestion` /
                    // `submitAnswers` at
                    // `AskUserQuestionPermissionRequest.tsx:407,565`.
                    let last_idx = (q.questions.len() as i32).saturating_sub(1);
                    if idx < last_idx {
                        // Re-set into the overlay (we own `q` here after
                        // the take()).
                        let mut q = q;
                        q.focus = QuestionFocus::Question(idx + 1);
                        state.ui.overlay = Some(Overlay::Question(q));
                        return;
                    }
                    let updated_input = build_answer_payload(&q);
                    let _ = command_tx
                        .send(UserCommand::ApprovalResponse {
                            request_id: q.request_id.clone(),
                            approved: true,
                            always_allow: false,
                            feedback: None,
                            updated_input: Some(updated_input),
                            permission_updates: vec![],
                            content_blocks: None,
                        })
                        .await;
                }
                QuestionFocus::ChatAboutThis => {
                    // TS `handleRespondToClaude`: rejection with the
                    // synthesized clarification prose. The model
                    // receives this as the rejection feedback and
                    // re-asks / clarifies.
                    let feedback = q.chat_about_this_feedback();
                    let _ = command_tx
                        .send(UserCommand::ApprovalResponse {
                            request_id: q.request_id.clone(),
                            approved: false,
                            always_allow: false,
                            feedback: Some(feedback),
                            updated_input: None,
                            permission_updates: vec![],
                            content_blocks: None,
                        })
                        .await;
                }
                QuestionFocus::SkipInterview => {
                    // Plan-mode-only. The renderer hides this footer
                    // item when `!is_in_plan_mode`, but Tab navigation
                    // also skips it — the focus enum should never carry
                    // SkipInterview when plan-mode is off. Defensive
                    // gate here in case future changes reach this arm
                    // outside plan mode.
                    if !q.is_in_plan_mode {
                        return;
                    }
                    let feedback = q.skip_interview_feedback();
                    let _ = command_tx
                        .send(UserCommand::ApprovalResponse {
                            request_id: q.request_id.clone(),
                            approved: false,
                            always_allow: false,
                            feedback: Some(feedback),
                            updated_input: None,
                            permission_updates: vec![],
                            content_blocks: None,
                        })
                        .await;
                }
            }
        }
        Some(Overlay::Settings(s)) => {
            // Only the Theme tab applies changes on Enter. Other tabs are
            // read-only (About) or populated asynchronously (OutputStyle,
            // Permissions). On Theme tab, theme rows persist to
            // ~/.coco/theme.json and the syntax row persists to settings.json.
            let mut s = s;
            if let crate::widgets::settings_panel::SettingsTab::Theme = s.active_tab {
                if let Some(choice) = s.selected_theme_choice().cloned() {
                    match state.ui.apply_theme_setting(choice.setting.clone()) {
                        Ok(()) => {
                            s.active_theme = choice.setting.clone();
                            match crate::theme::save_theme_setting(&choice.setting) {
                                Ok(path) => state.ui.add_toast(crate::state::ui::Toast::success(
                                    format!("Theme saved to {}", path.display()),
                                )),
                                Err(err) => state.ui.add_toast(crate::state::ui::Toast::error(
                                    format!("Failed to save theme: {err}"),
                                )),
                            }
                        }
                        Err(err) => state.ui.add_toast(crate::state::ui::Toast::error(format!(
                            "Failed to apply theme: {err}"
                        ))),
                    }
                } else if s.is_syntax_highlighting_selected() {
                    toggle_syntax_highlighting(state);
                    s.set_display_settings(state.ui.display_settings);
                }
            }
            // Keep settings open after selection — user may want to try
            // themes successively.
            state.ui.overlay = Some(Overlay::Settings(s));
            return;
        }
        // /memory file picker: create the file (mode `wx` semantics — silently
        // OK if exists), launch `$VISUAL || $EDITOR` on it, surface a toast
        // with the relative path. TS parity: `commands/memory/memory.tsx`'s
        // onSelect handler.
        Some(Overlay::MemoryDialog(m)) => {
            if let Some(entry) = m.entries.get(m.selected as usize).cloned() {
                open_memory_entry_async(state, &entry);
            }
            // overlay already taken; do not put back.
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
        // Permission overlay in choice mode: Enter commits the
        // currently-focused option (TS parity:
        // `ExitPlanModePermissionRequest.tsx:691-704`). The chosen
        // `value` is spliced into `updated_input` so the tool's
        // `execute()` can branch on it (e.g. ExitPlanMode reads
        // `user_choice == "yes-clear-context"` to flag history-clear).
        // Classic yes/no mode (no choices) falls into the dismiss
        // catch-all below — Enter is a no-op there, matching TS's
        // y/n-only confirmation prompt.
        Some(Overlay::Permission(ref p)) if p.choices.is_some() => {
            let chosen_is_no = p
                .choices
                .as_ref()
                .and_then(|cs| cs.get(p.selected_choice))
                .map(|c| c.value.as_str())
                == Some("no");
            let updated_input = build_choice_payload(p);
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: p.request_id.clone(),
                    approved: !chosen_is_no,
                    always_allow: false,
                    feedback: None,
                    updated_input,
                    permission_updates: vec![],
                    content_blocks: None,
                })
                .await;
            // Fall through to the queue-pop at end of fn.
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
            | Overlay::Transcript(_)
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
/// Skips the synthetic current-prompt row (no snapshot exists for "now").
pub(super) async fn request_diff_stats_if_rewind(
    state: &AppState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    if let Some(Overlay::Rewind(ref r)) = state.ui.overlay
        && let Some(msg) = r.messages.get(r.selected as usize)
        && !msg.is_current_prompt
    {
        let _ = command_tx
            .send(UserCommand::RequestDiffStats {
                message_id: msg.message_id.clone(),
            })
            .await;
    }
}

/// Cycle the focus within the Question overlay (Tab / Shift+Tab).
///
/// Order (TS `AskUserQuestionPermissionRequest.tsx`): Q0 → Q1 → … →
/// QN-1 → ChatAboutThis → SkipInterview (only when in plan mode) →
/// Q0 (wrap). `delta` is +1 for Tab, -1 for Shift+Tab. No-op when no
/// Question overlay is active.
pub(super) fn question_cycle_focus(state: &mut AppState, delta: i32) {
    use crate::state::QuestionFocus;
    let Some(Overlay::Question(ref mut q)) = state.ui.overlay else {
        return;
    };
    let q_count = q.questions.len() as i32;
    if q_count == 0 {
        return;
    }
    // Linearize the focus order so we can walk it as a Vec<QuestionFocus>.
    let mut order: Vec<QuestionFocus> = (0..q_count).map(QuestionFocus::Question).collect();
    order.push(QuestionFocus::ChatAboutThis);
    if q.is_in_plan_mode {
        order.push(QuestionFocus::SkipInterview);
    }
    let idx = order.iter().position(|f| *f == q.focus).unwrap_or(0) as i32;
    let len = order.len() as i32;
    let next = (idx + delta).rem_euclid(len) as usize;
    q.focus = order[next];
    // Keep `editing_notes` in sync with the new focus.
    if let QuestionFocus::Question(qi_idx) = q.focus
        && let Some(qi) = q.questions.get_mut(qi_idx as usize)
    {
        qi.editing_notes = qi
            .options
            .get(qi.selected as usize)
            .map(|o| o.label == crate::state::OTHER_OPTION_LABEL)
            .unwrap_or(false);
    }
}

/// Toggle the focused option's checked state in a multi-select question
/// (Space). Single-select and footer focus are no-ops. TS `MultiSelect`
/// onSpace handler in
/// `claude-code/src/components/permissions/AskUserQuestionPermissionRequest/QuestionView.tsx`.
pub(super) fn question_toggle_checked(state: &mut AppState) {
    use crate::state::QuestionFocus;
    let Some(Overlay::Question(ref mut q)) = state.ui.overlay else {
        return;
    };
    let QuestionFocus::Question(qi_idx) = q.focus else {
        return;
    };
    let Some(qi) = q.questions.get_mut(qi_idx as usize) else {
        return;
    };
    if !qi.multi_select {
        return;
    }
    let target = qi.selected;
    if let Some(pos) = qi.checked.iter().position(|i| *i == target) {
        qi.checked.swap_remove(pos);
    } else {
        qi.checked.push(target);
    }
}

/// Append a typed character into the focused question's `notes` buffer
/// when the Other option is focused (TS: text-input mode while
/// `__other__` selected). Returns `true` if the char was consumed.
/// Caller should fall back to the normal filter-input path when this
/// returns `false`.
pub(super) fn question_notes_input(state: &mut AppState, c: char) -> bool {
    use crate::state::QuestionFocus;
    let Some(Overlay::Question(ref mut q)) = state.ui.overlay else {
        return false;
    };
    let QuestionFocus::Question(qi_idx) = q.focus else {
        return false;
    };
    let Some(qi) = q.questions.get_mut(qi_idx as usize) else {
        return false;
    };
    if !qi.editing_notes {
        return false;
    }
    qi.notes.push(c);
    true
}

/// Backspace in the focused question's notes textarea. Returns `true`
/// if the keystroke was consumed.
pub(super) fn question_notes_backspace(state: &mut AppState) -> bool {
    use crate::state::QuestionFocus;
    let Some(Overlay::Question(ref mut q)) = state.ui.overlay else {
        return false;
    };
    let QuestionFocus::Question(qi_idx) = q.focus else {
        return false;
    };
    let Some(qi) = q.questions.get_mut(qi_idx as usize) else {
        return false;
    };
    if !qi.editing_notes {
        return false;
    }
    qi.notes.pop();
    true
}

/// Build the `{...original_input, user_choice}` payload shipped via
/// `UserCommand::ApprovalResponse.updated_input` when the user commits
/// a multi-choice permission selection. Carries every field the tool
/// originally supplied so its `execute()` can read both the new
/// `user_choice` field and the original args. Returns `None` when the
/// overlay has no choices or the cursor is out of range — caller falls
/// back to `updated_input: None`.
///
/// TS parity: `ExitPlanModePermissionRequest.tsx:691-704` — the
/// option's `value` is merged into the original tool input on commit.
fn build_choice_payload(p: &crate::state::PermissionOverlay) -> Option<serde_json::Value> {
    let choice = p.choices.as_ref()?.get(p.selected_choice)?;
    let mut payload = p
        .original_input
        .as_ref()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    payload.insert(
        "user_choice".into(),
        serde_json::Value::String(choice.value.clone()),
    );
    Some(serde_json::Value::Object(payload))
}

/// Build the `{...original_input, answers, annotations}` payload shipped
/// via `UserCommand::ApprovalResponse.updated_input`. Mirrors TS
/// `submitAnswers` at `AskUserQuestionPermissionRequest.tsx:407`.
fn build_answer_payload(q: &crate::state::QuestionOverlay) -> serde_json::Value {
    let mut answers = serde_json::Map::new();
    let mut annotations = serde_json::Map::new();

    for qi in &q.questions {
        // Pick checked indices (multi-select) or the focused one
        // (single-select). Multi-select with no toggles falls back to
        // the focused option so we never ship an empty answer for a
        // question that was actually shown.
        let chosen_indices: Vec<i32> = if qi.multi_select && !qi.checked.is_empty() {
            qi.checked.clone()
        } else {
            vec![qi.selected]
        };
        let labels: Vec<String> = chosen_indices
            .iter()
            .filter_map(|i| qi.options.get(*i as usize))
            .map(|o| {
                if o.label == crate::state::OTHER_OPTION_LABEL {
                    qi.notes.trim().to_string()
                } else {
                    o.label.clone()
                }
            })
            .filter(|s| !s.is_empty())
            .collect();
        let answer = labels.join(", ");
        answers.insert(qi.question.clone(), serde_json::Value::String(answer));

        // Annotation entry — preview from the focused option (TS
        // `selectedOption?.preview`) and notes from the typed buffer
        // (only when the focused option is NOT the Other sentinel,
        // since for Other the notes ARE the answer).
        let focused_opt = qi.options.get(qi.selected as usize);
        let is_other_focused = focused_opt
            .map(|o| o.label == crate::state::OTHER_OPTION_LABEL)
            .unwrap_or(false);
        let preview = focused_opt.and_then(|o| o.preview.as_ref());
        let notes_for_annotation = if is_other_focused {
            None
        } else {
            Some(qi.notes.trim()).filter(|s| !s.is_empty())
        };
        if preview.is_some() || notes_for_annotation.is_some() {
            let mut entry = serde_json::Map::new();
            if let Some(p) = preview {
                entry.insert("preview".into(), serde_json::Value::String(p.clone()));
            }
            if let Some(n) = notes_for_annotation {
                entry.insert("notes".into(), serde_json::Value::String(n.into()));
            }
            annotations.insert(qi.question.clone(), serde_json::Value::Object(entry));
        }
    }

    let mut payload = q.original_input.as_object().cloned().unwrap_or_default();
    payload.insert("answers".into(), serde_json::Value::Object(answers));
    if !annotations.is_empty() {
        payload.insert("annotations".into(), serde_json::Value::Object(annotations));
    }
    serde_json::Value::Object(payload)
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

/// Cycle the effort axis of the model picker by `delta`, clamped to
/// the focused entry's `supported_efforts`. No-op when the overlay
/// isn't a `ModelPicker` or when the focused model has no thinking
/// capability — the renderer hides the footer in that case so this
/// branch never triggers from the UI anyway.
pub(super) fn cycle_model_effort(state: &mut AppState, delta: i32) {
    let Some(Overlay::ModelPicker(m)) = &mut state.ui.overlay else {
        return;
    };
    let filtered: Vec<&ModelEntry> = m
        .entries
        .iter()
        .filter(|e| {
            m.filter.is_empty()
                || e.display_name
                    .to_lowercase()
                    .contains(&m.filter.to_lowercase())
                || e.provider_display
                    .to_lowercase()
                    .contains(&m.filter.to_lowercase())
        })
        .collect();
    let Some(entry) = filtered.get(m.selected as usize) else {
        return;
    };
    if !entry.unavailable_reasons.is_empty() {
        return;
    }
    if entry.supported_efforts.is_empty() {
        return;
    }
    let current_idx = m
        .effort
        .and_then(|e| entry.supported_efforts.iter().position(|&se| se == e))
        .unwrap_or(0) as i32;
    let n = entry.supported_efforts.len() as i32;
    let next_idx = (current_idx + delta).rem_euclid(n) as usize;
    m.effort = Some(entry.supported_efforts[next_idx]);
}

fn filtered_models(m: &ModelPickerOverlay) -> Vec<&ModelEntry> {
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

fn unavailable_summary(reasons: &[ProviderUnavailableReason]) -> Option<String> {
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

pub(super) fn toggle_syntax_highlighting(state: &mut AppState) {
    if let Some(source) = state
        .ui
        .display_settings
        .syntax_highlighting_editability
        .overriding_source()
    {
        state.ui.add_toast(Toast::warning(
            t!(
                "toast.syntax_highlighting_overridden",
                source = source.as_str()
            )
            .to_string(),
        ));
        return;
    }

    let next = state
        .ui
        .display_settings
        .with_syntax_highlighting(state.ui.display_settings.syntax_highlighting.toggle());

    let disabled = next.syntax_highlighting.is_disabled();
    match coco_config::global_config::write_user_setting(
        coco_config::settings::SYNTAX_HIGHLIGHTING_DISABLED_KEY,
        serde_json::json!(disabled),
    ) {
        Ok(path) => {
            state.ui.apply_display_settings(next);
            let status = crate::widgets::settings_panel::syntax_highlighting_status(
                next.syntax_highlighting,
            );
            let path_text = path.display().to_string();
            state.ui.add_toast(Toast::success(
                t!(
                    "toast.syntax_highlighting_saved",
                    status = status.as_str(),
                    path = path_text.as_str()
                )
                .to_string(),
            ));
        }
        Err(err) => state.ui.add_toast(Toast::error(
            t!(
                "toast.syntax_highlighting_save_failed",
                error = err.to_string().as_str()
            )
            .to_string(),
        )),
    }
}

/// Number of selectable items on the Settings active tab.
fn settings_item_count(s: &crate::widgets::settings_panel::SettingsPanelState) -> usize {
    use crate::widgets::settings_panel::SettingsTab;
    match s.active_tab {
        SettingsTab::Theme => s.theme_item_count(),
        SettingsTab::OutputStyle => s.output_styles.len(),
        SettingsTab::Permissions => s.permission_rules.len(),
        SettingsTab::About => 0,
    }
}

/// Open the memory file in `$VISUAL || $EDITOR` (or `vi` fallback) and
/// surface a toast about the result. TS parity:
/// `commands/memory/memory.tsx:47-78`'s `onSelect` handler:
///   1. `mkdir` parent dir (recursive),
///   2. `open(path, 'wx')` — create-exclusive; ignore EEXIST,
///   3. spawn `$VISUAL || $EDITOR <path>`,
///   4. emit `Opened memory file at <path>` system message.
///
/// Synchronous `mkdir` + `OpenOptions::create_new` are cheap and the
/// editor spawn is fire-and-forget (we don't wait for the editor to
/// close — same as TS, which uses `child_process.spawn` without await).
fn open_memory_entry_async(state: &mut AppState, entry: &MemoryDialogEntry) {
    if let Some(parent) = entry.path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        state.ui.add_toast(Toast::warning(
            t!("toast.memory_open_failed", error = e.to_string().as_str()).to_string(),
        ));
        return;
    }

    // `wx` semantics: create exclusively, but it's fine if the file
    // already exists — we just want it to be present before launching
    // the editor. `create_new(true)` errors with `AlreadyExists`;
    // swallow that, surface anything else.
    if let Err(e) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&entry.path)
        && e.kind() != std::io::ErrorKind::AlreadyExists
    {
        state.ui.add_toast(Toast::warning(
            t!("toast.memory_open_failed", error = e.to_string().as_str()).to_string(),
        ));
        return;
    }

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    match std::process::Command::new(&editor).arg(&entry.path).spawn() {
        Ok(_) => state.ui.add_toast(Toast::info(
            t!(
                "toast.memory_opened",
                path = entry.path.display().to_string().as_str()
            )
            .to_string(),
        )),
        Err(e) => state.ui.add_toast(Toast::warning(
            t!("toast.memory_open_failed", error = e.to_string().as_str()).to_string(),
        )),
    }
}

#[cfg(test)]
#[path = "overlay.test.rs"]
mod tests;
