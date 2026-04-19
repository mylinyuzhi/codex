//! Overlay constructors for the `Show*` commands.
//!
//! Each function builds the appropriate overlay struct and installs it via
//! `UiState::set_overlay`. Extracted from `update.rs` to keep the top-level
//! dispatch under 500 LoC.

use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::CommandOption;
use crate::state::CommandPaletteOverlay;
use crate::state::ExportFormat;
use crate::state::ExportOverlay;
use crate::state::GlobalSearchOverlay;
use crate::state::ModelOption;
use crate::state::ModelPickerOverlay;
use crate::state::Overlay;
use crate::state::QuickOpenOverlay;
use crate::state::SessionBrowserOverlay;
use crate::state::SessionOption;
use crate::state::Toast;
use crate::update_rewind;

/// Open the model picker, seeded from `available_models` (or current model).
pub(super) fn cycle_model(state: &mut AppState) {
    let model_list = if state.session.available_models.is_empty() {
        vec![state.session.model.clone()]
    } else {
        state.session.available_models.clone()
    };
    let models: Vec<ModelOption> = model_list
        .iter()
        .map(|m| ModelOption {
            id: m.clone(),
            label: m.clone(),
            description: None,
        })
        .collect();
    let selected = models
        .iter()
        .position(|m| m.id == state.session.model)
        .unwrap_or(0) as i32;
    state
        .ui
        .set_overlay(Overlay::ModelPicker(ModelPickerOverlay {
            models,
            filter: String::new(),
            selected,
        }));
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

/// Open the rewind overlay; surfaces a toast if there's nothing to rewind.
/// TS: MessageSelector useEffect loads diffStats on mount.
pub(super) async fn rewind(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    let overlay = update_rewind::build_rewind_overlay(state);
    if overlay.messages.is_empty() {
        state
            .ui
            .add_toast(Toast::info(t!("toast.no_rewind_messages").to_string()));
        return;
    }
    if let Some(msg) = overlay.messages.last() {
        let _ = command_tx
            .send(UserCommand::RequestDiffStats {
                message_id: msg.message_id.clone(),
            })
            .await;
    }
    state.ui.set_overlay(Overlay::Rewind(overlay));
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
