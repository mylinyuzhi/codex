//! Modal-surface action handlers — approve/deny/filter/navigate/confirm for
//! the full-screen modal surfaces, plus the update-layer entry points that
//! route a focused bottom-pane prompt first (`crate::bottom_pane`) and fall
//! through here.
//!
//! Factored out of `update.rs` to keep the top-level dispatch under 500 LoC.

use coco_types::PermissionMode;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::ExportFormat;
use crate::state::ModalState;
use crate::state::ModelEntry;
use crate::state::ModelPickerState;
use crate::state::ProviderUnavailableReason;
use crate::state::SessionBrowserState;
use crate::state::SessionOption;
use crate::state::ui::Toast;
use crate::update_rewind;
use coco_tui_ui::constants;

/// Handle `Approve` for the current prompt/modal. The focused bottom-pane
/// prompt wins; modal surfaces are the fallback.
pub(super) async fn approve(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    if crate::bottom_pane::route_approve(state, command_tx).await {
        return;
    }

    match state.ui.modal.as_ref() {
        Some(ModalState::BypassPermissions(_)) => {
            // Defense in depth: even after the user clicks Approve we
            // re-check the capability gate before flipping. The cycle
            // path already filters Bypass when the gate is off, but a
            // stale state (e.g. opened earlier in the session, gate
            // toggled since) shouldn't be able to escalate. Silently
            // dismiss without flipping when the gate is closed.
            if !state.session.bypass_permissions_available {
                state.ui.dismiss_modal();
                return;
            }
            state.session.permission_mode = PermissionMode::BypassPermissions;
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: PermissionMode::BypassPermissions,
                })
                .await;
            state.ui.dismiss_modal();
        }
        Some(ModalState::AutoModeOptIn(_)) => {
            state.session.permission_mode = PermissionMode::Auto;
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: PermissionMode::Auto,
                })
                .await;
            state.ui.dismiss_modal();
        }
        Some(ModalState::PluginHint(ph)) => {
            let response = ph.selected_response();
            let plugin_id = ph.plugin_id.clone();
            let plugin_name = ph.plugin_name.clone();
            apply_plugin_hint_response(state, command_tx, response, &plugin_id, &plugin_name).await;
            state.ui.dismiss_modal();
        }
        Some(ModalState::Trust(_) | ModalState::WorktreeExit(_)) => {
            state.ui.dismiss_modal();
        }
        _ => {
            state.ui.dismiss_modal();
        }
    }
}

/// Apply the user's plugin-hint decision. Records show-once (regardless of
/// yes/no), then routes the selected option:
///   - Install → dispatch `/plugin install <id>`.
///   - Dismiss → no further action.
///   - Disable → persist the opt-out flag.
///
/// TS: `useClaudeCodeHintRecommendation.tsx` handleResponse.
async fn apply_plugin_hint_response(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
    response: crate::state::PluginHintResponse,
    plugin_id: &str,
    plugin_name: &str,
) {
    use crate::state::PluginHintResponse;

    // Record show-once here, not at resolution-time — the dialog may have
    // been displaced by a higher-priority modal and never rendered.
    coco_plugins::mark_hint_plugin_shown(plugin_id);

    match response {
        PluginHintResponse::Install => {
            if let Ok(name) = crate::state::SlashCommandName::new("plugin") {
                let _ = command_tx
                    .send(UserCommand::ExecuteSlashCommand {
                        name,
                        args: format!("install {plugin_id}"),
                    })
                    .await;
            }
            state.ui.add_toast(crate::state::ui::Toast::info(
                t!("toast.plugin_hint_installing", name = plugin_name).to_string(),
            ));
        }
        PluginHintResponse::Disable => {
            coco_plugins::disable_hint_recommendations();
            state.ui.add_toast(crate::state::ui::Toast::info(
                t!("toast.plugin_hint_disabled").to_string(),
            ));
        }
        PluginHintResponse::Dismiss => {}
    }
}

/// Handle `Deny` for the current prompt/modal. The focused bottom-pane
/// prompt wins; modal surfaces are the fallback.
pub(super) async fn deny(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    if crate::bottom_pane::route_deny(state, command_tx).await {
        return;
    }

    match state.ui.modal.as_ref() {
        Some(ModalState::BypassPermissions(_)) | Some(ModalState::AutoModeOptIn(_)) => {
            // Declining bypass / auto opt-in keeps the current mode; just
            // close the modal — the status bar still shows the active mode.
            state.ui.dismiss_modal();
        }
        // Theme picker / settings (and any modal whose context maps Esc → Deny)
        // close through the shared helper so they restore the theme preview and
        // emit the same dismiss feedback as the `Cancel` route.
        _ => super::close_modal_with_feedback(state, command_tx).await,
    }
}

/// Push `c` into the current filterable state's filter string. A focused
/// bottom-pane prompt routes the keystroke first (Question prompts own
/// space/digit/free-text routing).
pub(super) fn filter(state: &mut AppState, c: char) {
    if crate::bottom_pane::route_filter(state, c) {
        return;
    }
    match state.ui.modal.as_mut() {
        Some(ModalState::ModelPicker(m)) => {
            m.filter.push(c);
            m.selected = 0;
        }
        Some(ModalState::SessionBrowser(s)) => {
            s.filter.push(c);
            s.selected = 0;
        }
        Some(ModalState::GlobalSearch(g)) => {
            g.query.push(c);
            g.selected = 0;
        }
        Some(ModalState::QuickOpen(q)) => {
            q.filter.push(c);
            q.selected = 0;
        }
        _ => {}
    }
}

/// Pop the last char from the current filterable state's filter string. A
/// focused bottom-pane prompt routes the keystroke first.
pub(super) fn filter_backspace(state: &mut AppState) {
    if crate::bottom_pane::route_filter_backspace(state) {
        return;
    }
    match state.ui.modal.as_mut() {
        Some(ModalState::ModelPicker(m)) => {
            m.filter.pop();
            m.selected = 0;
        }
        Some(ModalState::SessionBrowser(s)) => {
            s.filter.pop();
            s.selected = 0;
        }
        Some(ModalState::GlobalSearch(g)) => {
            g.query.pop();
            g.selected = 0;
        }
        Some(ModalState::QuickOpen(q)) => {
            q.filter.pop();
            q.selected = 0;
        }
        _ => {}
    }
}

/// Move selection by `delta` in the current list/scrollable state.
pub(super) fn nav(state: &mut AppState, delta: i32) {
    // Autocomplete takes precedence over (non-existent) state.
    if !state.ui.has_blocking_interaction()
        && let Some(ref mut sug) = state.ui.completion.active
    {
        if sug.items.is_empty() {
            sug.selected = 0;
        } else {
            let new = sug.selected as i32 + delta;
            sug.selected = new.clamp(0, sug.items.len() as i32 - 1) as usize;
        }
        return;
    }
    if crate::bottom_pane::route_nav(state, delta) {
        return;
    }

    let mut theme_preview: Option<crate::theme::ThemeSetting> = None;
    match state.ui.modal.as_mut() {
        Some(ModalState::ThemePicker(p)) => {
            let count = p.choices.len() as i32;
            let prev = p.selected;
            p.selected = (p.selected + delta).clamp(0, (count - 1).max(0));
            // Live preview: only when the focused row actually changed, capture
            // the theme and apply it in-memory *after* the borrow is released so
            // the whole picker recolors as the cursor moves (Esc restores
            // `original_setting`).
            if p.selected != prev {
                theme_preview = p
                    .choices
                    .get(p.selected as usize)
                    .map(|c| c.setting.clone());
            }
        }
        Some(ModalState::ModelPicker(m)) => {
            let count = filtered_models(m).len() as i32;
            m.selected = (m.selected + delta).clamp(0, (count - 1).max(0));
            // Re-derive effort from the newly-focused model's default
            // so the footer reflects "the model's preferred level"
            // unless the user has explicitly cycled past it.
            m.effort = filtered_models(m)
                .get(m.selected as usize)
                .and_then(|e| e.default_effort);
        }
        Some(ModalState::SessionBrowser(s)) => {
            let count = filtered_sessions(s).len() as i32;
            s.selected = (s.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(ModalState::GlobalSearch(g)) => {
            let count = g.results.len() as i32;
            g.selected = (g.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(ModalState::QuickOpen(q)) => {
            let count = q.files.len() as i32;
            q.selected = (q.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(ModalState::Export(e)) => {
            let count = e.formats.len() as i32;
            e.selected = (e.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(ModalState::Feedback(f)) => {
            let count = f.options.len() as i32;
            f.selected = (f.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(ModalState::PluginHint(ph)) => {
            let count = crate::state::PluginHintState::OPTION_COUNT;
            ph.selected = (ph.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(ModalState::Rewind(r)) => {
            update_rewind::handle_rewind_nav(r, delta);
        }
        Some(ModalState::DiffView(d)) => {
            d.scroll = (d.scroll + delta * constants::SCROLL_LINE_STEP).max(0);
        }
        Some(ModalState::TaskDetail(t)) => {
            t.scroll = (t.scroll + delta * constants::SCROLL_LINE_STEP).max(0);
        }
        Some(ModalState::Settings(s)) => {
            let count = settings_item_count(s) as i32;
            s.selected = (s.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(ModalState::MemoryDialog(m)) => {
            let count = m.entries.len() as i32;
            m.selected = (m.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(ModalState::TeamRoster(r)) => {
            let count = r.members.len() as i32;
            let next = (r.selected as i32 + delta).clamp(0, (count - 1).max(0));
            r.selected = next as usize;
        }
        Some(ModalState::CopyPicker(cp)) => {
            if delta < 0 {
                for _ in 0..delta.unsigned_abs() {
                    cp.move_up();
                }
            } else {
                for _ in 0..delta as u32 {
                    cp.move_down();
                }
            }
        }
        Some(
            ModalState::Help
            | ModalState::Doctor(_)
            | ModalState::Bridge(_)
            | ModalState::InvalidConfig(_),
        ) => {
            state.ui.help_scroll =
                (state.ui.help_scroll + delta * constants::SCROLL_LINE_STEP).max(0);
        }
        _ => {}
    }
    if let Some(setting) = theme_preview {
        let _ = state.ui.preview_theme_setting(setting);
    }
}

/// Confirm the currently selected item in the active prompt/modal.
pub(super) async fn confirm(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    if !state.ui.has_blocking_interaction() && state.ui.completion.active.is_some() {
        let _ = crate::completion::accept_suggestion(
            state,
            crate::completion::AcceptMode::AcceptSelected,
        );
        return;
    }

    if let Some(modal) = state.ui.take_modal() {
        match modal {
            ModalState::ModelPicker(m) => {
                if let Some(entry) = filtered_models(&m).get(m.selected as usize).copied() {
                    if let Some(summary) = unavailable_summary(&entry.unavailable_reasons) {
                        state.ui.restore_modal(ModalState::ModelPicker(m));
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
            ModalState::TeamRoster(_) => {
                // Mode cycling (Left/Right, Shift+Left/Right) already persisted
                // each change immediately (TS-faithful), so Enter just closes
                // the picker — `take_modal()` above already removed it.
            }
            ModalState::Rewind(mut r) => {
                // Capture the 1-based turn number for the protocol-level
                // `rewind/completed` notification before handle_rewind_confirm
                // mutates the state phase. `selected` is 0-based against the
                // filtered messages vec, so +1 yields the user-visible label.
                let rewound_turn = r.selected + 1;
                match update_rewind::handle_rewind_confirm(&mut r) {
                    update_rewind::ConfirmOutcome::Dispatch {
                        message_id,
                        restore,
                    } => {
                        // Keep the state open in `Confirming` phase while
                        // the rewind/summarize is in flight. TS:
                        // `MessageSelector.tsx:341-344`. `on_rewind_completed`
                        // dismisses the state when the engine notifies completion.
                        r.phase = crate::state::rewind::RewindPhase::Confirming;
                        state.ui.restore_modal(ModalState::Rewind(r));
                        let _ = command_tx
                            .send(UserCommand::Rewind {
                                message_id,
                                mode: crate::command::RewindMode::Explicit {
                                    restore_type: restore,
                                    rewound_turn,
                                },
                            })
                            .await;
                    }
                    update_rewind::ConfirmOutcome::Phase => {
                        // Phase transition without dispatch — put state back.
                        state.ui.restore_modal(ModalState::Rewind(r));
                    }
                    update_rewind::ConfirmOutcome::RequestDiffStats { message_id } => {
                        state.ui.restore_modal(ModalState::Rewind(r));
                        let _ = command_tx
                            .send(UserCommand::RequestDiffStats { message_id })
                            .await;
                    }
                    update_rewind::ConfirmOutcome::Dismiss => {
                        // Synthetic `(current)` row or preselected-Nevermind:
                        // close state (TS `MessageSelector.tsx:165` /
                        // line 186).
                        // (state already taken; do not put back.)
                    }
                }
                return;
            }
            ModalState::SessionBrowser(s) => {
                if let Some(session) = filtered_sessions(&s).get(s.selected as usize)
                    && let Ok(name) = crate::state::SlashCommandName::new("resume")
                {
                    let _ = command_tx
                        .send(UserCommand::ExecuteSlashCommand {
                            name,
                            args: session.id.clone(),
                        })
                        .await;
                }
            }
            ModalState::Export(e) => {
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
            ModalState::ThemePicker(p) => {
                // Standalone picker (TS): the focused theme is already applied
                // in-memory via live preview; Enter persists it and closes.
                if let Some(choice) = p.choices.get(p.selected.max(0) as usize).cloned() {
                    match state.ui.apply_theme_setting(choice.setting.clone()) {
                        Ok(()) => match crate::theme::save_theme_setting(&choice.setting) {
                            Ok(_path) => {
                                let messages = coco_messages::build_slash_command_messages(
                                    "theme",
                                    /*args*/ "",
                                    &format!("Theme set to {}", choice.label),
                                    /*is_sensitive*/ false,
                                );
                                let _ = command_tx
                                    .send(crate::command::UserCommand::PushSlashResult { messages })
                                    .await;
                            }
                            Err(err) => state.ui.add_toast(crate::state::ui::Toast::error(
                                format!("Failed to save theme: {err}"),
                            )),
                        },
                        Err(err) => state.ui.add_toast(crate::state::ui::Toast::error(format!(
                            "Failed to apply theme: {err}"
                        ))),
                    }
                }
                // Picker closes (modal already taken).
                return;
            }
            ModalState::Settings(s) => {
                // Only the Display tab toggles anything on Enter (syntax
                // highlighting → settings.json; copy-full-response → settings).
                // Theme selection moved to the standalone `/theme` picker.
                let mut s = s;
                if let crate::widgets::settings_panel::SettingsTab::Display = s.active_tab {
                    if s.is_syntax_highlighting_selected() {
                        toggle_syntax_highlighting(state);
                        s.set_display_settings(state.ui.display_settings.clone());
                    } else if s.is_copy_full_response_selected() {
                        toggle_copy_full_response(state);
                        s.set_display_settings(state.ui.display_settings.clone());
                    }
                }
                // Keep settings open after a toggle.
                state.ui.restore_modal(ModalState::Settings(s));
                return;
            }
            // /memory file picker: the TUI owns selection only. The CLI
            // bridge owns filesystem/editor effects and reports the result
            // through a TUI event so it can be rendered into transcript.
            ModalState::MemoryDialog(m) => {
                if let Some(entry) = m.entries.get(m.selected as usize).cloned() {
                    if entry.row_kind.is_file() {
                        let _ = command_tx
                            .send(UserCommand::OpenMemoryFile { path: entry.path })
                            .await;
                    } else {
                        state.ui.add_toast(Toast::warning(
                            t!("toast.memory_row_not_editable").to_string(),
                        ));
                        state.ui.restore_modal(ModalState::MemoryDialog(m));
                    }
                }
                // File rows dismiss after select; non-file rows are restored above.
                return;
            }
            ModalState::Transcript(t) => {
                state.ui.restore_modal(ModalState::Transcript(t));
                return;
            }
            ModalState::CopyPicker(cp) => {
                if let Some(message) = super::copy::confirm_picker_selection(state, cp) {
                    super::copy::enqueue_copy_output(message, command_tx);
                }
                state.ui.finish_taken_modal();
                return;
            }
            _ => {}
        }
        state.ui.finish_taken_modal();
        return;
    }

    let Some(prompt) = state.ui.take_prompt() else {
        return;
    };
    crate::bottom_pane::route_confirm(state, prompt, command_tx).await;
}

/// Send `RequestDiffStats` for the selected message when a Rewind state
/// is active — TS: MessageSelector useEffect recomputes on index change.
/// Skips the synthetic current-prompt row (no snapshot exists for "now").
pub(super) async fn request_diff_stats_if_rewind(
    state: &AppState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    if let Some(ModalState::Rewind(r)) = state.ui.modal.as_ref()
        && let Some(msg) = r.messages.get(r.selected as usize)
        && !msg.is_current_prompt
    {
        let _ = command_tx
            .send(UserCommand::RequestDiffStats {
                message_id: msg.message_id.to_string(),
            })
            .await;
    }
}

/// Rewind Esc: go back a phase before dismissing. Returns `true` if state
/// should be dismissed, `false` if a phase transition happened.
pub(super) fn rewind_cancel(state: &mut AppState) -> bool {
    if let Some(ModalState::Rewind(r)) = state.ui.modal.as_mut()
        && !update_rewind::handle_rewind_cancel(r)
    {
        return false;
    }
    true
}

// ── filter helpers ──

/// Cycle the effort axis of the model picker by `delta`, clamped to
/// the focused entry's `supported_efforts`. No-op when the state
/// isn't a `ModelPicker` or when the focused model has no thinking
/// capability — the renderer hides the footer in that case so this
/// branch never triggers from the UI anyway.
pub(super) fn cycle_model_effort(state: &mut AppState, delta: i32) {
    let Some(ModalState::ModelPicker(m)) = state.ui.modal.as_mut() else {
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

/// The four interactive permission modes the roster cycles through. The
/// auto-mode variants (Auto / DontAsk / Bubble) are intentionally excluded —
/// leaders set teammates to one of these four (matches the Shift+Tab cycle).
const ROSTER_MODE_ORDER: [coco_types::PermissionMode; 4] = [
    coco_types::PermissionMode::Default,
    coco_types::PermissionMode::AcceptEdits,
    coco_types::PermissionMode::Plan,
    coco_types::PermissionMode::BypassPermissions,
];

/// Advance `current` by `delta` steps through [`ROSTER_MODE_ORDER`] (wrapping).
fn next_roster_mode(current: coco_types::PermissionMode, delta: i32) -> coco_types::PermissionMode {
    let idx = ROSTER_MODE_ORDER
        .iter()
        .position(|m| *m == current)
        .unwrap_or(0) as i32;
    let n = ROSTER_MODE_ORDER.len() as i32;
    ROSTER_MODE_ORDER[(idx + delta).rem_euclid(n) as usize]
}

/// Cycle the FOCUSED teammate's mode by `delta` (Left/Right). Each teammate
/// carries its own mode, so divergent modes stay independent. Mutates the local
/// picker state for immediate UI feedback and returns `(name, new_mode)` to
/// persist (TS: cycling applies immediately). gap 8.
pub(super) fn team_roster_cycle_mode(
    state: &mut AppState,
    delta: i32,
) -> Option<(String, coco_types::PermissionMode)> {
    let Some(ModalState::TeamRoster(r)) = state.ui.modal.as_mut() else {
        return None;
    };
    let member = r.members.get_mut(r.selected)?;
    member.mode = next_roster_mode(member.mode, delta);
    Some((member.name.clone(), member.mode))
}

/// Cycle ALL teammates' modes in tandem by `delta` (Shift+Left/Right), mirroring
/// TS `cycleAllTeammateModes`: if the members' modes diverge, normalise every
/// member to `Default`; otherwise advance every member by `delta` from the
/// shared mode. Mutates the local picker state and returns the `(name, mode)`
/// updates to persist in one batch. Empty roster ⇒ no-op.
pub(super) fn team_roster_cycle_all_modes(
    state: &mut AppState,
    delta: i32,
) -> Vec<(String, coco_types::PermissionMode)> {
    let Some(ModalState::TeamRoster(r)) = state.ui.modal.as_mut() else {
        return Vec::new();
    };
    let Some(first_mode) = r.members.first().map(|m| m.mode) else {
        return Vec::new();
    };
    let all_same = r.members.iter().all(|m| m.mode == first_mode);
    let target = if all_same {
        next_roster_mode(first_mode, delta)
    } else {
        coco_types::PermissionMode::Default
    };
    r.members
        .iter_mut()
        .map(|m| {
            m.mode = target;
            (m.name.clone(), target)
        })
        .collect()
}

fn filtered_models(m: &ModelPickerState) -> Vec<&ModelEntry> {
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
        ProviderUnavailableReason::NotLoggedIn { provider } => t!(
            "dialog.model_picker_unavailable_not_logged_in",
            provider = provider.as_str()
        )
        .to_string(),
        ProviderUnavailableReason::NoModels => {
            t!("dialog.model_picker_unavailable_no_models").to_string()
        }
    }
}

fn filtered_sessions(s: &SessionBrowserState) -> Vec<&SessionOption> {
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
        .clone()
        .with_syntax_highlighting(state.ui.display_settings.syntax_highlighting.toggle());

    let disabled = next.syntax_highlighting.is_disabled();
    match coco_config::global_config::write_user_setting(
        coco_config::settings::SYNTAX_HIGHLIGHTING_DISABLED_KEY,
        serde_json::json!(disabled),
    ) {
        Ok(path) => {
            let status = crate::widgets::settings_panel::syntax_highlighting_status(
                next.syntax_highlighting,
            );
            state.ui.apply_display_settings(next);
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

fn toggle_copy_full_response(state: &mut AppState) {
    let enabled = !state.ui.display_settings.copy_full_response;
    let next = state
        .ui
        .display_settings
        .clone()
        .with_copy_full_response(enabled);

    match coco_config::global_config::write_user_setting(
        coco_config::settings::COPY_FULL_RESPONSE_KEY,
        serde_json::json!(enabled),
    ) {
        Ok(path) => {
            state.ui.apply_display_settings(next);
            let status = if enabled {
                t!("settings.enabled")
            } else {
                t!("settings.disabled")
            };
            let path_text = path.display().to_string();
            state.ui.add_toast(Toast::success(
                t!(
                    "toast.copy_full_response_saved",
                    status = status.as_ref(),
                    path = path_text.as_str()
                )
                .to_string(),
            ));
        }
        Err(err) => state.ui.add_toast(Toast::error(
            t!(
                "toast.copy_preference_save_failed",
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
        SettingsTab::Display => s.display_item_count(),
        SettingsTab::OutputStyle => s.output_styles.len(),
        SettingsTab::Permissions => s.permission_rules.len(),
        SettingsTab::About => 0,
    }
}

#[cfg(test)]
#[path = "interaction.test.rs"]
mod tests;
