//! Interaction action handlers — approve/deny/filter/navigate/confirm.
//!
//! Factored out of `update.rs` to keep the top-level dispatch under 500 LoC.
//! All helpers are internal to the update module.

use std::str::FromStr;

use coco_types::PermissionMode;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::ExportFormat;
use crate::state::ModalState;
use crate::state::ModelEntry;
use crate::state::ModelPickerState;
use crate::state::PanePromptState;
use crate::state::ProviderUnavailableReason;
use crate::state::SessionBrowserState;
use crate::state::SessionOption;
use crate::state::surface_payloads::PermissionAction;
use crate::state::ui::Toast;
use crate::update_rewind;
use coco_tui_ui::constants;

/// Handle `Approve` for the current prompt/modal.
pub(super) async fn approve(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    if let Some(prompt) = state.ui.interaction.active_prompt.as_ref() {
        match prompt {
            PanePromptState::Permission(p) => {
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
                tracing::info!(
                    target: "coco_tui::permission",
                    request_id = %p.request_id,
                    tool_name = %p.tool_name,
                    permission_decision = if approved { "approve" } else { "deny" },
                    always_allow = false,
                    multi_choice = p.choices.is_some(),
                    "user permission decision",
                );
                if let Err(e) = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id: p.request_id.clone(),
                        approved,
                        always_allow: false,
                        feedback: None,
                        updated_input,
                        permission_updates: vec![],
                        content_blocks: None,
                    })
                    .await
                {
                    tracing::warn!(
                        target: "coco_tui::permission",
                        error = %e,
                        "failed to dispatch ApprovalResponse (channel closed)",
                    );
                }
                state.ui.dismiss_prompt();
            }
            PanePromptState::SandboxPermission(s) => {
                tracing::info!(
                    target: "coco_tui::permission",
                    request_id = %s.request_id,
                    kind = "sandbox",
                    permission_decision = "approve",
                    "user sandbox permission decision",
                );
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
                state.ui.dismiss_prompt();
            }
            PanePromptState::McpServerApproval(m) => {
                tracing::info!(
                    target: "coco_tui::permission",
                    request_id = %m.request_id,
                    kind = "mcp_server",
                    permission_decision = "approve",
                    "user MCP server approval decision",
                );
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
                state.ui.dismiss_prompt();
            }
            PanePromptState::PlanEntry(_) => {
                // Entry: flip into Plan.
                state.toggle_plan_mode();
                let _ = command_tx
                    .send(UserCommand::SetPermissionMode {
                        mode: state.session.permission_mode,
                    })
                    .await;
                state.ui.dismiss_prompt();
            }
            PanePromptState::PlanExit(p) => {
                // Exit: target mode depends on which approval option the
                // user picked. `RestorePrePlan` defers the mode switch to
                // `ExitPlanModeTool::execute`, which writes the restored
                // mode onto `app_state.permission_mode` (source of truth);
                // the other variants explicitly set the target mode via
                // `SetPermissionMode` because the user's pick overrides
                // the stashed `pre_plan_mode`.
                //
                // Defense in depth: if the state somehow holds
                // `BypassPermissions` but the capability gate is off,
                // down-shift to `AcceptEdits` rather than silently
                // escalating. Normal paths can't reach this (the renderer
                // and cycle honor the gate) but a stale state is cheap
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
                state.ui.dismiss_prompt();
            }
            _ => state.ui.dismiss_prompt(),
        }
        return;
    }

    match state.ui.modal.as_ref() {
        Some(ModalState::BypassPermissions(_)) => {
            // Defense in depth: even after the user clicks Approve we
            // re-check the capability gate before flipping. The cycle
            // path already filters Bypass when the gate is off, but a
            // stale state (e.g. opened earlier in the session, gate
            // toggled since) shouldn't be able to escalate. Drops
            // through to a no-op + neutral toast so the user knows the
            // click was acknowledged without surprise.
            if !state.session.bypass_permissions_available {
                state.ui.add_toast(crate::state::ui::Toast::warning(
                    t!("toast.bypass_unavailable").to_string(),
                ));
                state.ui.dismiss_modal();
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
            state.ui.dismiss_modal();
        }
        Some(ModalState::AutoModeOptIn(_)) => {
            state.session.permission_mode = PermissionMode::Auto;
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: PermissionMode::Auto,
                })
                .await;
            state.ui.add_toast(crate::state::ui::Toast::info(
                t!("toast.auto_mode_enabled").to_string(),
            ));
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

/// Handle `Deny` for the current prompt/modal.
pub(super) async fn deny(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    if let Some(prompt) = state.ui.interaction.active_prompt.as_ref() {
        match prompt {
            PanePromptState::Permission(p) => {
                tracing::info!(
                    target: "coco_tui::permission",
                    request_id = %p.request_id,
                    tool_name = %p.tool_name,
                    permission_decision = "deny",
                    always_allow = false,
                    "user permission decision",
                );
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
                state.ui.dismiss_prompt();
            }
            PanePromptState::SandboxPermission(s) => {
                tracing::info!(
                    target: "coco_tui::permission",
                    request_id = %s.request_id,
                    kind = "sandbox",
                    permission_decision = "deny",
                    "user sandbox permission decision",
                );
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
                state.ui.dismiss_prompt();
            }
            PanePromptState::McpServerApproval(m) => {
                tracing::info!(
                    target: "coco_tui::permission",
                    request_id = %m.request_id,
                    kind = "mcp_server",
                    permission_decision = "deny",
                    "user MCP server approval decision",
                );
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
                state.ui.dismiss_prompt();
            }
            PanePromptState::PlanExit(p) => {
                // User rejected the plan. Surface a visible record in the
                // chat transcript — TS parity: `RejectedPlanMessage`
                // component renders the plan in a bordered block. Mode
                // stays in `Plan` (no mutation); the user can keep
                // refining or exit via the normal toggle. Routed through
                // engine round-trip (Commit 2) so the entry surfaces via
                // `MessageAppended` like every other system row.
                let plan = p.plan_content.clone().unwrap_or_default();
                let body = if plan.trim().is_empty() {
                    crate::i18n::t!("plan.rejected_empty").to_string()
                } else {
                    format!("{}\n\n{plan}", crate::i18n::t!("plan.rejected_header"),)
                };
                let _ = command_tx
                    .send(UserCommand::PushSystemMessage {
                        kind: crate::command::SystemPushKind::Informational {
                            level: coco_messages::SystemMessageLevel::Info,
                            title: String::new(),
                            message: body,
                        },
                    })
                    .await;
                state.ui.dismiss_prompt();
            }
            _ => state.ui.dismiss_prompt(),
        }
        return;
    }

    match state.ui.modal.as_ref() {
        Some(ModalState::BypassPermissions(_)) | Some(ModalState::AutoModeOptIn(_)) => {
            // Declining bypass / auto opt-in keeps the current mode.
            // A toast confirms the cancel so the user doesn't doubt
            // whether the Shift+Tab landed silently.
            let current = crate::update::permission_mode_label(state.session.permission_mode);
            state.ui.add_toast(crate::state::ui::Toast::info(
                crate::i18n::t!("toast.permission_mode_unchanged", mode = current.as_str())
                    .to_string(),
            ));
            state.ui.dismiss_modal();
        }
        // Theme picker / settings (and any modal whose context maps Esc → Deny)
        // close through the shared helper so they restore the theme preview and
        // emit the same dismiss feedback as the `Cancel` route.
        _ => super::close_modal_with_feedback(state, command_tx).await,
    }
}

/// Handle `ApproveAll` (always-allow) for permission prompts.
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
    if let Some(PanePromptState::Permission(p)) = state.ui.interaction.active_prompt.as_ref()
        && p.show_always_allow
    {
        let updates = always_allow_updates(
            &p.tool_name,
            p.original_input.as_ref(),
            &p.permission_suggestions,
        );
        tracing::info!(
            target: "coco_tui::permission",
            request_id = %p.request_id,
            tool_name = %p.tool_name,
            permission_decision = "approve",
            always_allow = true,
            rules = updates.len(),
            "user always-allow decision",
        );
        let _ = command_tx
            .send(UserCommand::ApprovalResponse {
                request_id: p.request_id.clone(),
                approved: true,
                always_allow: true,
                feedback: None,
                updated_input: None,
                permission_updates: updates,
                content_blocks: None,
            })
            .await;
        state.ui.dismiss_prompt();
    }
}

fn always_allow_updates(
    tool_name: &str,
    original_input: Option<&serde_json::Value>,
    permission_suggestions: &[coco_types::PermissionUpdate],
) -> Vec<coco_types::PermissionUpdate> {
    if !permission_suggestions.is_empty() {
        return permission_suggestions.to_vec();
    }
    if let Some(update) = read_path_allow_update(tool_name, original_input) {
        return vec![update];
    }
    vec![coco_types::PermissionUpdate::AddRules {
        rules: vec![coco_types::PermissionRule {
            source: coco_types::PermissionRuleSource::Session,
            behavior: coco_types::PermissionBehavior::Allow,
            value: coco_types::PermissionRuleValue {
                tool_pattern: tool_name.to_string(),
                rule_content: None,
            },
        }],
        destination: coco_types::PermissionUpdateDestination::Session,
    }]
}

fn read_path_allow_update(
    tool_name: &str,
    original_input: Option<&serde_json::Value>,
) -> Option<coco_types::PermissionUpdate> {
    let tool = coco_types::ToolName::from_str(tool_name).ok()?;
    if !matches!(
        tool,
        coco_types::ToolName::Read | coco_types::ToolName::Grep | coco_types::ToolName::Glob
    ) {
        return None;
    }
    let input = original_input?;
    let raw_path = match tool {
        coco_types::ToolName::Read => input.get("file_path").and_then(|v| v.as_str())?,
        coco_types::ToolName::Grep | coco_types::ToolName::Glob => {
            input.get("path").and_then(|v| v.as_str())?
        }
        _ => return None,
    };
    let dir = directory_for_permission_rule(raw_path)?;
    let rule_content = format!("{}/**", path_for_permission_rule(&dir));
    Some(coco_types::PermissionUpdate::AddRules {
        rules: vec![coco_types::PermissionRule {
            source: coco_types::PermissionRuleSource::Session,
            behavior: coco_types::PermissionBehavior::Allow,
            value: coco_types::PermissionRuleValue {
                tool_pattern: coco_types::ToolName::Read.as_str().to_string(),
                rule_content: Some(rule_content),
            },
        }],
        destination: coco_types::PermissionUpdateDestination::Session,
    })
}

fn directory_for_permission_rule(raw_path: &str) -> Option<std::path::PathBuf> {
    let path = shellexpand_read_path(raw_path);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let dir = if absolute.is_dir() {
        absolute
    } else {
        absolute.parent()?.to_path_buf()
    };
    (dir.parent().is_some()).then_some(dir)
}

fn shellexpand_read_path(raw_path: &str) -> std::path::PathBuf {
    if raw_path == "~" {
        return dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(raw_path));
    }
    if let Some(rest) = raw_path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    std::path::PathBuf::from(raw_path)
}

fn path_for_permission_rule(path: &std::path::Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if path.starts_with('/') {
        format!("/{path}")
    } else {
        path
    }
}

/// Handle `ClassifierAutoApprove` — background classifier approved the pending
/// request before the user responded.
pub(super) async fn classifier_auto_approve(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
    request_id: String,
) {
    if let Some(PanePromptState::Permission(p)) = state.ui.interaction.active_prompt.as_ref()
        && p.request_id == request_id
    {
        tracing::info!(
            target: "coco_tui::permission",
            request_id = %p.request_id,
            tool_name = %p.tool_name,
            permission_decision = "approve",
            source = "classifier",
            "classifier auto-approve",
        );
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
        state.ui.dismiss_prompt();
    }
}

/// Push `c` into the current filterable state's filter string.
pub(super) fn filter(state: &mut AppState, c: char) {
    // Question state specializes the keystroke routing: Space toggles
    // multi-select; printable chars edit the "Other" notes textarea
    // when that option is focused. Both consume the keystroke before
    // any filter logic. TS: `QuestionView.tsx` `onKeyDown` priority.
    if matches!(
        state.ui.interaction.active_prompt,
        Some(PanePromptState::Question(_))
    ) {
        if c == ' ' {
            question_toggle_checked(state);
            return;
        }
        if question_notes_input(state, c) {
            return;
        }
        return; // Question state has no filter — silently swallow.
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

/// Pop the last char from the current filterable state's filter string.
pub(super) fn filter_backspace(state: &mut AppState) {
    // Question state: when "Other" is focused, Backspace edits the
    // notes textarea. Otherwise no-op (Question has no filter).
    if matches!(
        state.ui.interaction.active_prompt,
        Some(PanePromptState::Question(_))
    ) {
        question_notes_backspace(state);
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
    if let Some(prompt) = state.ui.interaction.active_prompt.as_mut() {
        match prompt {
            PanePromptState::Question(q) => {
                if let crate::state::QuestionFocus::Question(idx) = q.focus
                    && let Some(qi) = q.questions.get_mut(idx as usize)
                {
                    let count = qi.options.len() as i32;
                    let next = (qi.selected + delta).clamp(0, (count - 1).max(0));
                    qi.selected = next;
                    qi.editing_notes = qi
                        .options
                        .get(next as usize)
                        .map(|o| o.label == crate::state::OTHER_OPTION_LABEL)
                        .unwrap_or(false);
                }
            }
            PanePromptState::PlanExit(p) => {
                let order = crate::state::PlanExitTarget::available(
                    state.session.bypass_permissions_available,
                );
                let current_idx = order.iter().position(|t| *t == p.next_mode).unwrap_or(0) as i32;
                let len = order.len() as i32;
                let new_idx = ((current_idx + delta).rem_euclid(len)) as usize;
                p.next_mode = order[new_idx];
            }
            PanePromptState::Permission(p) => {
                let count = p
                    .choices
                    .as_ref()
                    .map(Vec::len)
                    .unwrap_or_else(|| p.classic_action_count()) as i32;
                if count > 0 {
                    let current = p.selected_choice as i32;
                    let next = (current + delta).rem_euclid(count);
                    p.selected_choice = next as usize;
                }
            }
            PanePromptState::PlanApproval(p) => {
                if delta != 0 {
                    p.toggle_focus();
                }
            }
            _ => {}
        }
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
    match prompt {
        PanePromptState::Question(q) => {
            use crate::state::QuestionFocus;
            match q.focus {
                QuestionFocus::Question(idx) => {
                    let last_idx = (q.questions.len() as i32).saturating_sub(1);
                    if idx < last_idx {
                        let mut q = q;
                        q.focus = QuestionFocus::Question(idx + 1);
                        state.ui.restore_prompt(PanePromptState::Question(q));
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
                    if !q.is_in_plan_mode {
                        state.ui.restore_prompt(PanePromptState::Question(q));
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
        PanePromptState::PlanApproval(p) => {
            let _ = command_tx
                .send(UserCommand::PlanApprovalResponse {
                    request_id: p.request_id.clone(),
                    teammate_agent: p.from.clone(),
                    approved: p.is_approve_focused(),
                    feedback: None,
                })
                .await;
        }
        PanePromptState::PlanExit(p) => {
            let target = p.next_mode.resolve().unwrap_or(PermissionMode::Default);
            state.session.permission_mode = target;
            let _ = command_tx
                .send(UserCommand::SetPermissionMode { mode: target })
                .await;
        }
        PanePromptState::Permission(ref p) => {
            let (approved, always_allow, updated_input, permission_updates) = if p.choices.is_some()
            {
                let chosen_is_no = p
                    .choices
                    .as_ref()
                    .and_then(|cs| cs.get(p.selected_choice))
                    .map(|c| c.value.as_str())
                    == Some("no");
                (!chosen_is_no, false, build_choice_payload(p), vec![])
            } else {
                match p.selected_classic_action() {
                    PermissionAction::ApproveOnce => (true, false, None, vec![]),
                    PermissionAction::AlwaysAllow => (
                        true,
                        true,
                        None,
                        always_allow_updates(
                            &p.tool_name,
                            p.original_input.as_ref(),
                            &p.permission_suggestions,
                        ),
                    ),
                    PermissionAction::Deny => (false, false, None, vec![]),
                }
            };
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: p.request_id.clone(),
                    approved,
                    always_allow,
                    feedback: None,
                    updated_input,
                    permission_updates,
                    content_blocks: None,
                })
                .await;
        }
        _ => {}
    }
    state.ui.finish_taken_prompt();
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

/// Cycle the focus within the Question state (Tab / Shift+Tab).
///
/// Order (TS `AskUserQuestionPermissionRequest.tsx`): Q0 → Q1 → … →
/// QN-1 → ChatAboutThis → SkipInterview (only when in plan mode) →
/// Q0 (wrap). `delta` is +1 for Tab, -1 for Shift+Tab. No-op when no
/// Question state is active.
pub(super) fn question_cycle_focus(state: &mut AppState, delta: i32) {
    use crate::state::QuestionFocus;
    let Some(PanePromptState::Question(q)) = state.ui.interaction.active_prompt.as_mut() else {
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
    let Some(PanePromptState::Question(q)) = state.ui.interaction.active_prompt.as_mut() else {
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
    let Some(PanePromptState::Question(q)) = state.ui.interaction.active_prompt.as_mut() else {
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
    let Some(PanePromptState::Question(q)) = state.ui.interaction.active_prompt.as_mut() else {
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
/// state has no choices or the cursor is out of range — caller falls
/// back to `updated_input: None`.
///
/// TS parity: `ExitPlanModePermissionRequest.tsx:691-704` — the
/// option's `value` is merged into the original tool input on commit.
fn build_choice_payload(p: &crate::state::PermissionPromptState) -> Option<serde_json::Value> {
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
fn build_answer_payload(q: &crate::state::QuestionPromptState) -> serde_json::Value {
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
