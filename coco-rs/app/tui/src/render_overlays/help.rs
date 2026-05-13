//! Help overlay renderer — lists the spec keybindings.
//!
//! The shortcut column is rendered dynamically: for entries whose
//! `action` is `Some(...)`, [`crate::keybinding_resolver::display_for`]
//! resolves the live binding (so user customizations in
//! `~/.coco/keybindings.json` show through). When no action is bound
//! (TUI-only shortcuts like F1 or F6, or actions the user explicitly
//! null-bound) the static `fallback` is used.

use coco_keybindings::KeybindingAction;
use ratatui::prelude::Color;

use crate::i18n::t;
use crate::keybinding_bridge::KeybindingContext as TuiContext;
use crate::state::AppState;
use crate::theme::Theme;

/// One help-overlay row.
struct HelpEntry {
    /// Action whose live binding fills the shortcut column. `None`
    /// means the shortcut is TUI-only (no schema entry); always use
    /// `fallback`.
    action: Option<KeybindingAction>,
    /// Shortcut text used when `action` is `None` or the action isn't
    /// bound in the resolver's defaults.
    fallback: &'static str,
    /// i18n key for the description column.
    description_key: &'static str,
}

/// Width (in chars) of the shortcut column. Picked to fit the widest
/// default chord (`ctrl+shift+f`, 12 chars) + 2 spaces.
const SHORTCUT_COLUMN_WIDTH: usize = 14;

/// All rows shown in the help overlay. Order matters — preserved
/// from the prior static layout.
///
/// Fallbacks use the same canonical lowercase form
/// `coco_keybindings::keystroke_to_display_string` produces (so rows
/// with bound actions visually match TUI-only rows).
fn entries() -> &'static [HelpEntry] {
    use KeybindingAction::*;
    // Allocated once per call; the const-array form is awkward
    // with Option<KeybindingAction> because action variants aren't
    // const-constructible without `const fn`.
    static ROWS: std::sync::OnceLock<Vec<HelpEntry>> = std::sync::OnceLock::new();
    ROWS.get_or_init(|| {
        vec![
            // TUI-only: no TS schema entry for plan-mode toggle.
            HelpEntry {
                action: None,
                fallback: "tab",
                description_key: "help.desc.toggle_plan_mode",
            },
            HelpEntry {
                action: Some(ChatCycleMode),
                fallback: "shift+tab",
                description_key: "help.desc.cycle_permission_mode",
            },
            // coco-rs extension: in Chat context Ctrl+T cycles the
            // Main role's thinking effort. `app:toggleTodos` stays
            // reachable from non-Chat contexts (Picker, Settings, …)
            // but the help overlay is Chat-context only so we surface
            // the thinking cycle here.
            HelpEntry {
                action: Some(ChatCycleThinking),
                fallback: "ctrl+t",
                description_key: "help.desc.cycle_thinking_level",
            },
            // TUI-only: cycle model is bound to Ctrl+M in the legacy
            // cascade; TS schema doesn't have an equivalent action.
            HelpEntry {
                action: None,
                fallback: "ctrl+m",
                description_key: "help.desc.cycle_model",
            },
            HelpEntry {
                action: Some(AppInterrupt),
                fallback: "ctrl+c",
                description_key: "help.desc.interrupt",
            },
            HelpEntry {
                action: Some(AppRedraw),
                fallback: "ctrl+l",
                description_key: "help.desc.clear_screen",
            },
            // TUI-only: kill-to-end-of-line lives in the input-edit
            // cascade, not in TS schema.
            HelpEntry {
                action: None,
                fallback: "ctrl+k",
                description_key: "help.desc.kill_to_end_of_line",
            },
            HelpEntry {
                action: None,
                fallback: "ctrl+y",
                description_key: "help.desc.yank_killed_text",
            },
            HelpEntry {
                action: Some(ChatExternalEditor),
                fallback: "ctrl+e",
                description_key: "help.desc.external_editor",
            },
            // Command palette is TUI-only; TS uses Ctrl+R for history
            // search instead.
            HelpEntry {
                action: None,
                fallback: "ctrl+p",
                description_key: "help.desc.command_palette",
            },
            HelpEntry {
                action: Some(ChatStash),
                fallback: "ctrl+s",
                description_key: "help.desc.stash_input_draft",
            },
            HelpEntry {
                action: Some(ChatKillAgents),
                fallback: "ctrl+f",
                description_key: "help.desc.kill_all_agents",
            },
            HelpEntry {
                action: Some(ChatFastMode),
                fallback: "alt+o",
                description_key: "help.desc.toggle_fast_mode",
            },
            HelpEntry {
                action: Some(AppGlobalSearch),
                fallback: "ctrl+shift+f",
                description_key: "help.desc.global_search",
            },
            HelpEntry {
                action: Some(AppToggleTranscript),
                fallback: "ctrl+o",
                description_key: "help.desc.toggle_transcript",
            },
            HelpEntry {
                action: Some(AppToggleTeammatePreview),
                fallback: "ctrl+shift+o",
                description_key: "help.desc.toggle_teammate_preview",
            },
            HelpEntry {
                action: Some(AppQuickOpen),
                fallback: "ctrl+shift+p",
                description_key: "help.desc.quick_open_file",
            },
            // TUI-only.
            HelpEntry {
                action: None,
                fallback: "ctrl+w",
                description_key: "help.desc.context_window",
            },
            HelpEntry {
                action: None,
                fallback: "f6",
                description_key: "help.desc.focus_next_panel",
            },
            HelpEntry {
                action: Some(AppExit),
                fallback: "ctrl+q",
                description_key: "help.desc.quit",
            },
            HelpEntry {
                action: None,
                fallback: "?/f1",
                description_key: "help.desc.this_help",
            },
            HelpEntry {
                action: None,
                fallback: "Esc",
                description_key: "help.desc.close_overlay",
            },
            HelpEntry {
                action: None,
                fallback: "PageUp/Down",
                description_key: "help.desc.scroll",
            },
            // Prompt-mode prefixes — typed inline, not bound to a key.
            // Listed last so the keybinding rows above stay together.
            HelpEntry {
                action: None,
                fallback: "!cmd",
                description_key: "help.desc.bash_mode",
            },
            HelpEntry {
                action: None,
                fallback: "#note",
                description_key: "help.desc.memory_mode",
            },
            HelpEntry {
                action: None,
                fallback: "/cmd",
                description_key: "help.desc.slash_commands",
            },
            HelpEntry {
                action: None,
                fallback: "@path",
                description_key: "help.desc.file_mention",
            },
        ]
    })
}

pub(super) fn help_content(state: &AppState, theme: &Theme) -> (String, String, Color) {
    let body = entries()
        .iter()
        .map(|e| render_row(state, e))
        .collect::<Vec<_>>()
        .join("\n");
    (t!("help.title").to_string(), body, theme.primary)
}

fn render_row(state: &AppState, entry: &HelpEntry) -> String {
    let shortcut = entry
        .action
        .as_ref()
        .and_then(|a| state.ui.kb_handle.display_for(a, TuiContext::Chat))
        .unwrap_or_else(|| entry.fallback.to_string());
    let description = t!(entry.description_key).to_string();
    format!("{shortcut:<SHORTCUT_COLUMN_WIDTH$} {description}")
}
