//! Default keybindings — TS port of `keybindings/defaultBindings.ts`.
//!
//! Block-for-block mirror with two exceptions:
//!
//! * Platform-conditional keys (`IMAGE_PASTE_KEY`, `MODE_CYCLE_KEY`) use
//!   `cfg!(target_os = ...)` instead of TS runtime detection.
//! * Feature-gated TS blocks (`KAIROS`, `QUICK_SEARCH`, `TERMINAL_PANEL`,
//!   `MESSAGE_ACTIONS`, `VOICE_MODE`) are intentionally skipped — they
//!   depend on Anthropic-internal infrastructure (GrowthBook, etc.) that
//!   coco-rs doesn't ship. Re-add behind a Cargo feature when the
//!   underlying capability lands.
//!
//! TS source: `keybindings/defaultBindings.ts:32-340`.

use std::collections::BTreeMap;

use crate::KeybindingAction;
use crate::KeybindingBlock;
use crate::KeybindingContext;
use crate::KeybindingsConfig;

/// Image-paste shortcut. TS: `defaultBindings.ts:15` —
/// Windows uses `alt+v` because `ctrl+v` is system paste.
#[cfg(target_os = "windows")]
const IMAGE_PASTE_KEY: &str = "alt+v";
#[cfg(not(target_os = "windows"))]
const IMAGE_PASTE_KEY: &str = "ctrl+v";

/// Permission-mode cycle shortcut. TS: `defaultBindings.ts:30` — falls
/// back to `meta+m` on Windows without VT mode (we always use
/// `shift+tab` on non-Windows; on Windows we conservatively assume VT
/// mode is available because Node ≥22.17 / Bun ≥1.2.23 enable it).
const MODE_CYCLE_KEY: &str = "shift+tab";

fn make_block<const N: usize>(
    context: KeybindingContext,
    entries: [(&str, KeybindingAction); N],
) -> KeybindingBlock {
    let mut bindings = BTreeMap::new();
    for (chord, action) in entries {
        bindings.insert(chord.to_string(), Some(action));
    }
    KeybindingBlock { context, bindings }
}

/// Return the full default `KeybindingsConfig`. Hot-load merge order:
/// defaults first, user bindings later (last-wins).
///
/// Mirrors `DEFAULT_BINDINGS` (`defaultBindings.ts:32-340`).
pub fn default_config() -> KeybindingsConfig {
    KeybindingsConfig {
        schema: None,
        docs: None,
        bindings: default_blocks(),
    }
}

/// Return the default blocks as a `Vec<KeybindingBlock>`.
///
/// Block order matches TS so user expectations (e.g. iteration in
/// `/help`) match.
pub fn default_blocks() -> Vec<KeybindingBlock> {
    vec![
        // ── Global — defaultBindings.ts:33-62 ─────────────────────────
        make_block(
            KeybindingContext::Global,
            [
                // ctrl+c / ctrl+d use special double-press handling at
                // the dispatch layer; they are listed here so the
                // resolver can render them in /help, but the validator
                // refuses user attempts to rebind them (P5 reserved).
                ("ctrl+c", KeybindingAction::AppInterrupt),
                ("ctrl+d", KeybindingAction::AppExit),
                ("ctrl+l", KeybindingAction::AppRedraw),
                ("ctrl+t", KeybindingAction::AppToggleTodos),
                ("ctrl+o", KeybindingAction::AppToggleTranscript),
                ("ctrl+shift+o", KeybindingAction::AppToggleTeammatePreview),
                ("ctrl+r", KeybindingAction::HistorySearch),
                // TS gates these on QUICK_SEARCH; coco-rs doesn't gate
                // (the surfaces are part of the base TUI), so they ship
                // unconditionally. Mirrors TS bindings.
                ("ctrl+shift+f", KeybindingAction::AppGlobalSearch),
                ("ctrl+shift+p", KeybindingAction::AppQuickOpen),
            ],
        ),
        // ── Chat — defaultBindings.ts:63-98 ───────────────────────────
        make_block(
            KeybindingContext::Chat,
            [
                ("escape", KeybindingAction::ChatCancel),
                // ctrl+x prefix avoids shadowing readline editing keys.
                ("ctrl+x ctrl+k", KeybindingAction::ChatKillAgents),
                (MODE_CYCLE_KEY, KeybindingAction::ChatCycleMode),
                ("meta+p", KeybindingAction::ChatModelPicker),
                ("meta+o", KeybindingAction::ChatFastMode),
                ("meta+t", KeybindingAction::ChatThinkingToggle),
                // coco-rs extension (no TS counterpart): Ctrl+T in the
                // Chat context cycles the Main role's thinking effort
                // forward through the active model's
                // `supported_thinking_levels`. Shadows the Global
                // `ctrl+t → app:toggleTodos` while the user is at the
                // input; the global binding stays reachable from other
                // contexts (Picker, Settings, Scrollable) where the
                // task panel toggle still makes sense.
                ("ctrl+t", KeybindingAction::ChatCycleThinking),
                ("enter", KeybindingAction::ChatSubmit),
                ("up", KeybindingAction::HistoryPrevious),
                ("down", KeybindingAction::HistoryNext),
                // Undo: dual binding for legacy + kitty-keyboard
                // protocol terminals (defaultBindings.ts:80-81).
                ("ctrl+_", KeybindingAction::ChatUndo),
                ("ctrl+shift+-", KeybindingAction::ChatUndo),
                ("ctrl+x ctrl+e", KeybindingAction::ChatExternalEditor),
                ("ctrl+g", KeybindingAction::ChatExternalEditor),
                ("ctrl+s", KeybindingAction::ChatStash),
                (IMAGE_PASTE_KEY, KeybindingAction::ChatImagePaste),
            ],
        ),
        // ── Autocomplete — defaultBindings.ts:99-107 ──────────────────
        make_block(
            KeybindingContext::Autocomplete,
            [
                ("tab", KeybindingAction::AutocompleteAccept),
                ("escape", KeybindingAction::AutocompleteDismiss),
                ("up", KeybindingAction::AutocompletePrevious),
                ("down", KeybindingAction::AutocompleteNext),
            ],
        ),
        // ── Settings — defaultBindings.ts:108-129 ─────────────────────
        make_block(
            KeybindingContext::Settings,
            [
                ("escape", KeybindingAction::ConfirmNo),
                ("up", KeybindingAction::SelectPrevious),
                ("down", KeybindingAction::SelectNext),
                ("k", KeybindingAction::SelectPrevious),
                ("j", KeybindingAction::SelectNext),
                ("ctrl+p", KeybindingAction::SelectPrevious),
                ("ctrl+n", KeybindingAction::SelectNext),
                ("space", KeybindingAction::SelectAccept),
                ("enter", KeybindingAction::SettingsClose),
                ("/", KeybindingAction::SettingsSearch),
                ("r", KeybindingAction::SettingsRetry),
            ],
        ),
        // ── Confirmation — defaultBindings.ts:130-149 ─────────────────
        make_block(
            KeybindingContext::Confirmation,
            [
                ("y", KeybindingAction::ConfirmYes),
                ("n", KeybindingAction::ConfirmNo),
                ("enter", KeybindingAction::ConfirmYes),
                ("escape", KeybindingAction::ConfirmNo),
                ("up", KeybindingAction::ConfirmPrevious),
                ("down", KeybindingAction::ConfirmNext),
                ("tab", KeybindingAction::ConfirmNextField),
                ("space", KeybindingAction::ConfirmToggle),
                ("shift+tab", KeybindingAction::ConfirmCycleMode),
                ("ctrl+e", KeybindingAction::ConfirmToggleExplanation),
                ("ctrl+d", KeybindingAction::PermissionToggleDebug),
            ],
        ),
        // ── Tabs — defaultBindings.ts:150-159 ─────────────────────────
        make_block(
            KeybindingContext::Tabs,
            [
                ("tab", KeybindingAction::TabsNext),
                ("shift+tab", KeybindingAction::TabsPrevious),
                ("right", KeybindingAction::TabsNext),
                ("left", KeybindingAction::TabsPrevious),
            ],
        ),
        // ── Transcript — defaultBindings.ts:160-170 ───────────────────
        make_block(
            KeybindingContext::Transcript,
            [
                ("ctrl+e", KeybindingAction::TranscriptToggleShowAll),
                ("ctrl+c", KeybindingAction::TranscriptExit),
                ("escape", KeybindingAction::TranscriptExit),
                ("q", KeybindingAction::TranscriptExit),
            ],
        ),
        // ── HistorySearch — defaultBindings.ts:171-180 ────────────────
        make_block(
            KeybindingContext::HistorySearch,
            [
                ("ctrl+r", KeybindingAction::HistorySearchNext),
                ("escape", KeybindingAction::HistorySearchAccept),
                ("tab", KeybindingAction::HistorySearchAccept),
                ("ctrl+c", KeybindingAction::HistorySearchCancel),
                ("enter", KeybindingAction::HistorySearchExecute),
            ],
        ),
        // ── Task — defaultBindings.ts:181-188 ─────────────────────────
        make_block(
            KeybindingContext::Task,
            [("ctrl+b", KeybindingAction::TaskBackground)],
        ),
        // ── ThemePicker — defaultBindings.ts:189-194 ──────────────────
        make_block(
            KeybindingContext::ThemePicker,
            [("ctrl+t", KeybindingAction::ThemeToggleSyntaxHighlighting)],
        ),
        // ── Scroll (internal) — defaultBindings.ts:195-213 ────────────
        make_block(
            KeybindingContext::Scroll,
            [
                ("pageup", KeybindingAction::ScrollPageUp),
                ("pagedown", KeybindingAction::ScrollPageDown),
                // wheelup/wheeldown have no crossterm equivalent at the
                // KeyEvent layer; they ship as TS aliases for line scroll
                // here for completeness.
                ("ctrl+home", KeybindingAction::ScrollTop),
                ("ctrl+end", KeybindingAction::ScrollBottom),
                ("ctrl+shift+c", KeybindingAction::SelectionCopy),
                ("cmd+c", KeybindingAction::SelectionCopy),
            ],
        ),
        // ── Help — defaultBindings.ts:214-219 ─────────────────────────
        make_block(
            KeybindingContext::Help,
            [("escape", KeybindingAction::HelpDismiss)],
        ),
        // ── Attachments — defaultBindings.ts:220-231 ──────────────────
        make_block(
            KeybindingContext::Attachments,
            [
                ("right", KeybindingAction::AttachmentsNext),
                ("left", KeybindingAction::AttachmentsPrevious),
                ("backspace", KeybindingAction::AttachmentsRemove),
                ("delete", KeybindingAction::AttachmentsRemove),
                ("down", KeybindingAction::AttachmentsExit),
                ("escape", KeybindingAction::AttachmentsExit),
            ],
        ),
        // ── Footer — defaultBindings.ts:232-245 ───────────────────────
        make_block(
            KeybindingContext::Footer,
            [
                ("up", KeybindingAction::FooterUp),
                ("ctrl+p", KeybindingAction::FooterUp),
                ("down", KeybindingAction::FooterDown),
                ("ctrl+n", KeybindingAction::FooterDown),
                ("right", KeybindingAction::FooterNext),
                ("left", KeybindingAction::FooterPrevious),
                ("enter", KeybindingAction::FooterOpenSelected),
                ("escape", KeybindingAction::FooterClearSelection),
            ],
        ),
        // ── MessageSelector — defaultBindings.ts:246-265 ──────────────
        make_block(
            KeybindingContext::MessageSelector,
            [
                ("up", KeybindingAction::MessageSelectorUp),
                ("down", KeybindingAction::MessageSelectorDown),
                ("k", KeybindingAction::MessageSelectorUp),
                ("j", KeybindingAction::MessageSelectorDown),
                ("ctrl+p", KeybindingAction::MessageSelectorUp),
                ("ctrl+n", KeybindingAction::MessageSelectorDown),
                ("ctrl+up", KeybindingAction::MessageSelectorTop),
                ("shift+up", KeybindingAction::MessageSelectorTop),
                ("meta+up", KeybindingAction::MessageSelectorTop),
                ("shift+k", KeybindingAction::MessageSelectorTop),
                ("ctrl+down", KeybindingAction::MessageSelectorBottom),
                ("shift+down", KeybindingAction::MessageSelectorBottom),
                ("meta+down", KeybindingAction::MessageSelectorBottom),
                ("shift+j", KeybindingAction::MessageSelectorBottom),
                ("enter", KeybindingAction::MessageSelectorSelect),
            ],
        ),
        // ── DiffDialog — defaultBindings.ts:296-308 ───────────────────
        make_block(
            KeybindingContext::DiffDialog,
            [
                ("escape", KeybindingAction::DiffDismiss),
                ("left", KeybindingAction::DiffPreviousSource),
                ("right", KeybindingAction::DiffNextSource),
                ("up", KeybindingAction::DiffPreviousFile),
                ("down", KeybindingAction::DiffNextFile),
                ("enter", KeybindingAction::DiffViewDetails),
            ],
        ),
        // ── ModelPicker — defaultBindings.ts:309-316 ──────────────────
        make_block(
            KeybindingContext::ModelPicker,
            [
                ("left", KeybindingAction::ModelPickerDecreaseEffort),
                ("right", KeybindingAction::ModelPickerIncreaseEffort),
            ],
        ),
        // ── Select — defaultBindings.ts:317-330 ───────────────────────
        make_block(
            KeybindingContext::Select,
            [
                ("up", KeybindingAction::SelectPrevious),
                ("down", KeybindingAction::SelectNext),
                ("j", KeybindingAction::SelectNext),
                ("k", KeybindingAction::SelectPrevious),
                ("ctrl+n", KeybindingAction::SelectNext),
                ("ctrl+p", KeybindingAction::SelectPrevious),
                ("enter", KeybindingAction::SelectAccept),
                ("escape", KeybindingAction::SelectCancel),
            ],
        ),
        // ── Plugin — defaultBindings.ts:331-339 ───────────────────────
        make_block(
            KeybindingContext::Plugin,
            [
                ("space", KeybindingAction::PluginToggle),
                ("i", KeybindingAction::PluginInstall),
            ],
        ),
    ]
}

#[cfg(test)]
#[path = "defaults.test.rs"]
mod tests;
