//! Overlay constructors for the `Show*` commands.
//!
//! Each function builds the appropriate overlay struct and installs it via
//! `UiState::set_overlay`. Extracted from `update.rs` to keep the top-level
//! dispatch under 500 LoC.

use tokio::sync::mpsc;

use coco_types::ModelRole;

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
use crate::state::QuickOpenOverlay;
use crate::state::SessionBrowserOverlay;
use crate::state::SessionOption;
use crate::update_rewind;

/// Open the model picker for the `Main` role, seeded from the builtin
/// registry. Provider attribution is inferred from `model_id` prefix
/// because [`coco_config::builtin_models_partial`] doesn't carry it
/// (it's keyed only by `model_id`). The picker is open-only: changing
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

/// Build the picker entries for `role`. Pulls metadata from
/// `coco_config::builtin_models_partial()` and synthesises the
/// provider id from the model-id prefix.
///
/// Restricting to builtins is intentional for now: surfacing
/// user-registered models requires plumbing `ModelRegistry` through
/// the TUI session state, which is a separate change. Builtins cover
/// every provider coco-rs ships with.
pub(super) fn build_model_entries(state: &AppState, role: ModelRole) -> Vec<ModelEntry> {
    // Optional allow-list from settings.available_models. When set,
    // limit the picker to those ids so users locked to a subset
    // (corporate policy, billing constraints) don't see ineligible
    // models.
    let allowlist: Option<&[String]> = if state.session.available_models.is_empty() {
        None
    } else {
        Some(state.session.available_models.as_slice())
    };

    let current_for_role = current_model_for_role(state, role);

    let mut entries: Vec<ModelEntry> = coco_config::builtin_models_partial()
        .iter()
        .filter(|(model_id, _)| {
            allowlist
                .map(|a| a.iter().any(|s| s == *model_id))
                .unwrap_or(true)
        })
        .map(|(model_id, partial)| {
            let (provider, provider_display) = infer_provider(model_id);
            let supported_efforts = partial
                .supported_thinking_levels
                .as_ref()
                .map(|levels| levels.iter().map(|l| l.effort).collect())
                .unwrap_or_default();
            ModelEntry {
                provider: provider.to_string(),
                provider_display: provider_display.to_string(),
                model_id: model_id.clone(),
                display_name: partial
                    .display_name
                    .clone()
                    .unwrap_or_else(|| model_id.clone()),
                context_window: partial.context_window.map(|t| t.get() as i64),
                supported_efforts,
                default_effort: partial.default_thinking_level,
                is_current_for_role: current_for_role
                    .as_deref()
                    .map(|m| m == model_id)
                    .unwrap_or(false),
            }
        })
        .collect();

    // Sort by (provider_display, display_name) so providers cluster
    // and within a provider models are alphabetic — stable for both
    // rendering (headers fall between sections) and snapshot tests.
    entries.sort_by(|a, b| {
        a.provider_display
            .cmp(&b.provider_display)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    entries
}

/// Lookup the model id currently bound to `role`. For now only `Main`
/// has a live mirror (`state.session.model`); other roles fall through
/// to the Main id so the picker still surfaces a sensible "current"
/// marker. Wire per-role state when the engine exposes it.
fn current_model_for_role(state: &AppState, _role: ModelRole) -> Option<String> {
    Some(state.session.model.clone())
}

/// Map a builtin `model_id` to its canonical provider. Builtin
/// registry doesn't carry a provider field (entries are keyed only
/// by id), so this lookup is the seam between display and persistence.
fn infer_provider(model_id: &str) -> (&'static str, &'static str) {
    if model_id.starts_with("claude-") {
        ("anthropic", "Anthropic")
    } else if model_id.starts_with("gpt-") || model_id.starts_with('o') {
        ("openai", "OpenAI")
    } else if model_id.starts_with("gemini-") {
        ("google", "Google")
    } else if model_id.starts_with("deepseek-") {
        ("deepseek", "DeepSeek")
    } else {
        ("other", "Other")
    }
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
    const ORDER: [ModelRole; 9] = [
        ModelRole::Main,
        ModelRole::Fast,
        ModelRole::Compact,
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
    let cmd_list = if state.session.available_commands.is_empty() {
        vec![
            ("help".to_string(), Some(t!("palette.help").to_string())),
            ("clear".to_string(), Some(t!("palette.clear").to_string())),
            (
                "compact".to_string(),
                Some(t!("palette.compact").to_string()),
            ),
            ("config".to_string(), Some(t!("palette.config").to_string())),
            (
                "copy".to_string(),
                Some(t!("palette.copy_last").to_string()),
            ),
            ("doctor".to_string(), Some(t!("palette.doctor").to_string())),
            ("diff".to_string(), Some(t!("palette.diff").to_string())),
            ("login".to_string(), Some(t!("palette.login").to_string())),
            ("mcp".to_string(), Some(t!("palette.mcp").to_string())),
            (
                "session".to_string(),
                Some(t!("palette.session").to_string()),
            ),
        ]
    } else {
        state.session.available_commands.clone()
    };
    let commands: Vec<CommandOption> = cmd_list
        .iter()
        .map(|(name, desc)| CommandOption {
            name: name.clone(),
            description: desc.clone(),
        })
        .collect();
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
        crate::widgets::settings_panel::SettingsPanelState::new(),
    ));
}

#[cfg(test)]
#[path = "show.test.rs"]
mod tests;
