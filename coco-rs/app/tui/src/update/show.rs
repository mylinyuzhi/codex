//! Surface constructors for the `Show*` commands.
//!
//! Each function builds the appropriate prompt/modal state. Extracted from `update.rs` to keep the top-level
//! dispatch under 500 LoC.

use std::collections::HashSet;

use coco_types::ModelRole;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::state::ActiveSuggestions;
use crate::state::AppState;
use crate::state::ComposerPopupState;
use crate::state::ExportFormat;
use crate::state::ExportState;
use crate::state::GlobalSearchState;
use crate::state::ModalState;
use crate::state::ModelEntry;
use crate::state::ModelPickerState;
use crate::state::ProviderUnavailableReason;
use crate::state::QuickOpenState;
use crate::state::SessionBrowserState;
use crate::state::SessionOption;
use crate::state::SlashPopupState;
use crate::state::SuggestionKind;
use crate::update_rewind;

/// Open the model picker for the `Main` role, seeded from the
/// session-frozen model catalog. The picker is open-only: changing
/// roles inside it cycles via `update::interaction::cycle_model_role`.
///
/// `pub(crate)` so the `OpenModelPicker` TuiOnlyEvent handler can
/// open the picker without round-tripping through the keybind layer.
pub(crate) fn cycle_model(state: &mut AppState) {
    let role = ModelRole::Main;
    let entries = build_model_entries(state, role);
    let selected = entries
        .iter()
        .position(|e| e.is_current_for_role)
        .unwrap_or(0) as i32;
    let effort = entries
        .get(selected as usize)
        .and_then(|e| e.default_effort);
    state
        .ui
        .show_modal(ModalState::ModelPicker(ModelPickerState {
            role,
            entries,
            filter: String::new(),
            selected,
            effort,
        }));
}

/// Build the picker entries for `role` from the session-frozen
/// `model_catalog`. The catalog covers all three resolution layers
/// (L0 builtin + L1 `~/.coco/models.json` + L2 per-provider
/// overrides), seeded by `tui_runner` at session start. When the
/// catalog is empty, the picker shows only provider-level availability
/// rows; tests and mock pre-bootstrap paths must seed catalog entries
/// explicitly.
pub(super) fn build_model_entries(state: &AppState, role: ModelRole) -> Vec<ModelEntry> {
    let allowlist: Option<&[String]> = if state.session.available_models.is_empty() {
        None
    } else {
        Some(state.session.available_models.as_slice())
    };
    let current_for_role = current_model_for_role(state, role);

    let entries = state
        .session
        .model_catalog
        .iter()
        .filter(|entry| {
            allowlist
                .map(|a| a.iter().any(|s| s == &entry.model_id))
                .unwrap_or(true)
        })
        .map(|entry| ModelEntry {
            provider: entry.provider.clone(),
            provider_display: entry.provider_display.clone(),
            model_id: entry.model_id.clone(),
            display_name: entry.display_name.clone(),
            context_window: entry.context_window,
            supported_efforts: entry.supported_efforts.clone(),
            default_effort: entry.default_effort,
            is_current_for_role: current_for_role
                .as_ref()
                .map(|(p, m)| p == &entry.provider && m == &entry.model_id)
                .unwrap_or(false),
            unavailable_reasons: Vec::new(),
        })
        .collect();
    apply_provider_statuses(state, entries)
}

fn apply_provider_statuses(state: &AppState, mut entries: Vec<ModelEntry>) -> Vec<ModelEntry> {
    for entry in &mut entries {
        if let Some(status) = state.session.provider_statuses.get(&entry.provider) {
            entry.unavailable_reasons = status.unavailable_reasons.clone();
        }
    }

    let providers_with_entries: HashSet<String> =
        entries.iter().map(|entry| entry.provider.clone()).collect();
    for (provider, status) in &state.session.provider_statuses {
        if providers_with_entries.contains(provider) {
            continue;
        }
        let mut reasons = status.unavailable_reasons.clone();
        if !reasons.contains(&ProviderUnavailableReason::NoModels) {
            reasons.push(ProviderUnavailableReason::NoModels);
        }
        entries.push(ModelEntry {
            provider: provider.clone(),
            provider_display: status.provider_display.clone(),
            model_id: String::new(),
            display_name: status.provider_display.clone(),
            context_window: None,
            supported_efforts: Vec::new(),
            default_effort: None,
            is_current_for_role: false,
            unavailable_reasons: reasons,
        });
    }

    sort_model_entries(&mut entries);
    entries
}

fn sort_model_entries(entries: &mut [ModelEntry]) {
    entries.sort_by(|a, b| {
        a.provider_display
            .cmp(&b.provider_display)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
}

/// Lookup the `(provider, model_id)` pair currently bound to `role`.
/// Reads `state.session.model_by_role` (the live mirror populated by
/// `ModelRoleChanged` and seeded at session start); falls back to the
/// Main pair when a role has no binding (matches the engine's own
/// `ModelRoles` fallback chain).
fn current_model_for_role(state: &AppState, role: ModelRole) -> Option<(String, String)> {
    if let Some(b) = state.session.model_by_role.get(&role) {
        return Some((b.provider.clone(), b.model_id.clone()));
    }
    if state.session.provider.is_empty() || state.session.model.is_empty() {
        return None;
    }
    Some((state.session.provider.clone(), state.session.model.clone()))
}

/// Cycle the picker's target role by `delta`, rebuilding entries for
/// the new role. Called from the keybind layer via Tab / Shift+Tab.
pub(super) fn cycle_model_role(state: &mut AppState, delta: i32) {
    let role = match state.ui.modal.as_ref() {
        Some(ModalState::ModelPicker(m)) => m.role,
        _ => return,
    };
    let next = next_role(role, delta);
    let entries = build_model_entries(state, next);
    let selected = entries
        .iter()
        .position(|e| e.is_current_for_role)
        .unwrap_or(0) as i32;
    let effort = entries
        .get(selected as usize)
        .and_then(|e| e.default_effort);
    if let Some(ModalState::ModelPicker(m)) = state.ui.modal.as_mut() {
        m.role = next;
        m.entries = entries;
        m.filter.clear();
        m.selected = selected;
        m.effort = effort;
    }
}

/// Canonical role order for the picker pill. Wraps with `rem_euclid`
/// so Tab from `Subagent` returns to `Main`.
fn next_role(current: ModelRole, delta: i32) -> ModelRole {
    const ORDER: [ModelRole; 8] = [
        ModelRole::Main,
        ModelRole::Fast,
        ModelRole::Plan,
        ModelRole::Explore,
        ModelRole::Review,
        ModelRole::HookAgent,
        ModelRole::Memory,
        ModelRole::Subagent,
    ];
    let idx = ORDER.iter().position(|r| *r == current).unwrap_or(0) as i32;
    let n = ORDER.len() as i32;
    ORDER[((idx + delta).rem_euclid(n)) as usize]
}

/// Open the command palette, seeded only from the command registry snapshot.
pub(super) fn command_palette(state: &mut AppState) {
    let items = state
        .session
        .available_commands
        .iter()
        .map(|cmd| crate::widgets::suggestion_popup::SuggestionItem {
            label: format!("/{}", cmd.name),
            description: cmd.description.clone(),
            metadata: None,
        })
        .collect();
    let suggestions = ActiveSuggestions {
        kind: SuggestionKind::SlashCommand,
        items,
        selected: 0,
        query: String::new(),
        trigger_pos: 0,
    };
    state.ui.active_suggestions = Some(suggestions);
    state.ui.interaction.popup = Some(ComposerPopupState::Slash(SlashPopupState));
}

/// Open the session browser populated from `saved_sessions`.
pub(super) fn session_browser(state: &mut AppState) {
    let sessions: Vec<SessionOption> = state
        .session
        .saved_sessions
        .iter()
        .map(|s| SessionOption {
            id: s.id.clone(),
            label: s.label.clone(),
            message_count: s.message_count,
            created_at: s.created_at.clone(),
        })
        .collect();
    state
        .ui
        .show_modal(ModalState::SessionBrowser(SessionBrowserState {
            sessions,
            filter: String::new(),
            selected: 0,
        }));
}

/// Open the global search state with an empty query.
pub(super) fn global_search(state: &mut AppState) {
    state
        .ui
        .show_modal(ModalState::GlobalSearch(GlobalSearchState {
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            is_searching: false,
        }));
}

/// Open the quick-open file picker.
pub(super) fn quick_open(state: &mut AppState) {
    state.ui.show_modal(ModalState::QuickOpen(QuickOpenState {
        filter: String::new(),
        files: Vec::new(),
        selected: 0,
    }));
}

/// Open the export state with the available formats.
pub(super) fn export(state: &mut AppState) {
    state.ui.show_modal(ModalState::Export(ExportState {
        formats: vec![
            ExportFormat::Markdown,
            ExportFormat::Json,
            ExportFormat::Text,
        ],
        selected: 0,
    }));
}

/// Open the rewind state pre-anchored to `message_id`, jumping
/// straight to the RestoreOptions confirm screen. TS:
/// `setMessageSelectorPreselect(raw); setIsMessageSelectorVisible(true)`
/// (`screens/REPL.tsx:3783-3784`). Falls back to the standard picker
/// when the id doesn't match any selectable message.
pub(super) async fn rewind_for(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
    message_id: String,
) {
    let rewind = update_rewind::build_rewind_state_for(state, Some(&message_id));
    let load_all_stats =
        rewind.file_history_enabled && rewind.messages.iter().any(|m| !m.is_current_prompt);
    let row_ids: Vec<String> = if load_all_stats {
        rewind
            .messages
            .iter()
            .filter(|m| !m.is_current_prompt)
            .map(|m| m.message_id.clone())
            .collect()
    } else {
        Vec::new()
    };
    state.ui.show_modal(ModalState::Rewind(rewind));
    for id in row_ids {
        let _ = command_tx
            .send(UserCommand::RequestDiffStats { message_id: id })
            .await;
    }
}

/// Open the rewind state; renders inline empty-state when nothing is
/// rewindable. TS: MessageSelector useEffect loads diffStats per row on
/// mount (`MessageSelector.tsx:285-312`); we mirror that by firing
/// `RequestDiffStats` for every row instead of just the selected one.
pub(super) async fn rewind(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    let rewind = update_rewind::build_rewind_state(state);
    let load_all_stats = rewind.file_history_enabled && !rewind.messages.is_empty();
    let row_ids: Vec<String> = if load_all_stats {
        // Skip the synthetic current-prompt row (empty message_id);
        // there is no snapshot to fetch for "now".
        rewind
            .messages
            .iter()
            .filter(|m| !m.is_current_prompt)
            .map(|m| m.message_id.clone())
            .collect()
    } else {
        Vec::new()
    };
    state.ui.show_modal(ModalState::Rewind(rewind));
    for id in row_ids {
        let _ = command_tx
            .send(UserCommand::RequestDiffStats { message_id: id })
            .await;
    }
}

/// Open the doctor/diagnostics state.
pub(super) fn doctor(state: &mut AppState) {
    state
        .ui
        .show_modal(ModalState::Doctor(crate::state::DoctorState {
            checks: Vec::new(),
        }));
}

/// Open the tabbed settings state (theme, output style, permissions, about).
pub(super) fn settings(state: &mut AppState) {
    state.ui.show_modal(ModalState::Settings(
        crate::widgets::settings_panel::SettingsPanelState::new(
            &state.ui.theme_state,
            state.ui.display_settings,
        ),
    ));
}

#[cfg(test)]
#[path = "show.test.rs"]
mod tests;
