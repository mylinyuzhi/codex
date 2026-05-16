//! Overlay constructors for the `Show*` commands.
//!
//! Each function builds the appropriate overlay struct and installs it via
//! `UiState::set_overlay`. Extracted from `update.rs` to keep the top-level
//! dispatch under 500 LoC.

use std::collections::HashSet;

use coco_types::ModelRole;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::CommandOption;
use crate::state::CommandPaletteOverlay;
use crate::state::ExportFormat;
use crate::state::ExportOverlay;
use crate::state::GlobalSearchOverlay;
use crate::state::ModelEntry;
use crate::state::ModelPickerOverlay;
use crate::state::Overlay;
use crate::state::ProviderUnavailableReason;
use crate::state::QuickOpenOverlay;
use crate::state::SessionBrowserOverlay;
use crate::state::SessionOption;
use crate::update_rewind;

/// Open the model picker for the `Main` role, seeded from the
/// session-frozen model catalog. The picker is open-only: changing
/// roles inside it cycles via `update::overlay::cycle_model_role`.
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
        .set_overlay(Overlay::ModelPicker(ModelPickerOverlay {
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
    if !matches!(state.ui.overlay, Some(Overlay::ModelPicker(_))) {
        return;
    }
    let role = match &state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => m.role,
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
    if let Some(Overlay::ModelPicker(m)) = &mut state.ui.overlay {
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

/// Open the command palette, seeded from `available_commands` (or defaults).
pub(super) fn command_palette(state: &mut AppState) {
    let commands: Vec<CommandOption> = if state.session.available_commands.is_empty() {
        // Fallback list — used only when the registry snapshot never
        // landed (smoke tests, bare `AppState::new`). Production paths
        // seed `available_commands` from `CommandRegistry` in
        // `tui_runner::run`.
        vec![
            ("help", t!("palette.help")),
            ("clear", t!("palette.clear")),
            ("compact", t!("palette.compact")),
            ("config", t!("palette.config")),
            ("copy", t!("palette.copy_last")),
            ("doctor", t!("palette.doctor")),
            ("diff", t!("palette.diff")),
            ("login", t!("palette.login")),
            ("mcp", t!("palette.mcp")),
            ("session", t!("palette.session")),
        ]
        .into_iter()
        .map(|(name, desc)| CommandOption {
            name: name.to_string(),
            description: Some(desc.to_string()),
        })
        .collect()
    } else {
        state
            .session
            .available_commands
            .iter()
            .map(|cmd| CommandOption {
                name: cmd.name.clone(),
                description: cmd.description.clone(),
            })
            .collect()
    };
    state
        .ui
        .set_overlay(Overlay::CommandPalette(CommandPaletteOverlay {
            commands,
            filter: String::new(),
            selected: 0,
        }));
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
        .set_overlay(Overlay::SessionBrowser(SessionBrowserOverlay {
            sessions,
            filter: String::new(),
            selected: 0,
        }));
}

/// Open the global search overlay with an empty query.
pub(super) fn global_search(state: &mut AppState) {
    state
        .ui
        .set_overlay(Overlay::GlobalSearch(GlobalSearchOverlay {
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            is_searching: false,
        }));
}

/// Open the quick-open file picker.
pub(super) fn quick_open(state: &mut AppState) {
    state.ui.set_overlay(Overlay::QuickOpen(QuickOpenOverlay {
        filter: String::new(),
        files: Vec::new(),
        selected: 0,
    }));
}

/// Open the export overlay with the available formats.
pub(super) fn export(state: &mut AppState) {
    state.ui.set_overlay(Overlay::Export(ExportOverlay {
        formats: vec![
            ExportFormat::Markdown,
            ExportFormat::Json,
            ExportFormat::Text,
        ],
        selected: 0,
    }));
}

/// Open the rewind overlay pre-anchored to `message_id`, jumping
/// straight to the RestoreOptions confirm screen. TS:
/// `setMessageSelectorPreselect(raw); setIsMessageSelectorVisible(true)`
/// (`screens/REPL.tsx:3783-3784`). Falls back to the standard picker
/// when the id doesn't match any selectable message.
pub(super) async fn rewind_for(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
    message_id: String,
) {
    let overlay = update_rewind::build_rewind_overlay_for(state, Some(&message_id));
    let load_all_stats =
        overlay.file_history_enabled && overlay.messages.iter().any(|m| !m.is_current_prompt);
    let row_ids: Vec<String> = if load_all_stats {
        overlay
            .messages
            .iter()
            .filter(|m| !m.is_current_prompt)
            .map(|m| m.message_id.clone())
            .collect()
    } else {
        Vec::new()
    };
    state.ui.set_overlay(Overlay::Rewind(overlay));
    for id in row_ids {
        let _ = command_tx
            .send(UserCommand::RequestDiffStats { message_id: id })
            .await;
    }
}

/// Open the rewind overlay; renders inline empty-state when nothing is
/// rewindable. TS: MessageSelector useEffect loads diffStats per row on
/// mount (`MessageSelector.tsx:285-312`); we mirror that by firing
/// `RequestDiffStats` for every row instead of just the selected one.
pub(super) async fn rewind(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    let overlay = update_rewind::build_rewind_overlay(state);
    let load_all_stats = overlay.file_history_enabled && !overlay.messages.is_empty();
    let row_ids: Vec<String> = if load_all_stats {
        // Skip the synthetic current-prompt row (empty message_id);
        // there is no snapshot to fetch for "now".
        overlay
            .messages
            .iter()
            .filter(|m| !m.is_current_prompt)
            .map(|m| m.message_id.clone())
            .collect()
    } else {
        Vec::new()
    };
    state.ui.set_overlay(Overlay::Rewind(overlay));
    for id in row_ids {
        let _ = command_tx
            .send(UserCommand::RequestDiffStats { message_id: id })
            .await;
    }
}

/// Open the doctor/diagnostics overlay.
pub(super) fn doctor(state: &mut AppState) {
    state
        .ui
        .set_overlay(Overlay::Doctor(crate::state::DoctorOverlay {
            checks: Vec::new(),
        }));
}

/// Open the tabbed settings overlay (theme, output style, permissions, about).
pub(super) fn settings(state: &mut AppState) {
    state.ui.set_overlay(Overlay::Settings(
        crate::widgets::settings_panel::SettingsPanelState::new(
            &state.ui.theme_state,
            state.ui.display_settings,
        ),
    ));
}

#[cfg(test)]
#[path = "show.test.rs"]
mod tests;
