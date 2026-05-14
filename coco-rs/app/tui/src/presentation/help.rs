//! Help overlay presentation.

use coco_keybindings::KeybindingAction;
use ratatui::prelude::Color;

use crate::i18n::t;
use crate::keybinding_bridge::KeybindingContext as TuiContext;
use crate::state::AppState;
use crate::theme::Theme;

struct HelpEntry {
    action: Option<KeybindingAction>,
    fallback: &'static str,
    description_key: &'static str,
}

const SHORTCUT_COLUMN_WIDTH: usize = 14;

fn entries() -> &'static [HelpEntry] {
    use KeybindingAction::*;
    static ROWS: std::sync::OnceLock<Vec<HelpEntry>> = std::sync::OnceLock::new();
    ROWS.get_or_init(|| {
        vec![
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
            HelpEntry {
                action: Some(ChatCycleThinking),
                fallback: "ctrl+t",
                description_key: "help.desc.cycle_thinking_level",
            },
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

pub(crate) fn help_content(state: &AppState, theme: &Theme) -> (String, String, Color) {
    let body = entries()
        .iter()
        .map(|entry| render_row(state, entry))
        .collect::<Vec<_>>()
        .join("\n");
    (t!("help.title").to_string(), body, theme.primary)
}

fn render_row(state: &AppState, entry: &HelpEntry) -> String {
    let shortcut = entry
        .action
        .as_ref()
        .and_then(|action| state.ui.kb_handle.display_for(action, TuiContext::Chat))
        .unwrap_or_else(|| entry.fallback.to_string());
    let description = t!(entry.description_key).to_string();
    format!("{shortcut:<SHORTCUT_COLUMN_WIDTH$} {description}")
}

#[cfg(test)]
#[path = "help.test.rs"]
mod tests;
