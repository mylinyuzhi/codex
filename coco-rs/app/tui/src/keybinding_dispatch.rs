//! `KeybindingAction` → `TuiCommand` dispatch.
//!
//! The resolver in `coco-keybindings` produces a typed
//! [`KeybindingAction`] from a key event + active context stack. This
//! module owns the TUI-side mapping from those actions to
//! [`TuiCommand`]s, including state-dependent dispatch (`Enter` while
//! streaming queues input, etc.) — the part that can't live in the
//! pure-logic resolver.
//!
//! Returns `None` when the action has no TUI-side handler. The caller
//! treats that as "swallow without effect" so unmapped TS actions
//! don't fall through to the legacy hardcoded cascade.

use coco_keybindings::KeybindingAction;

use crate::events::TuiCommand;
use crate::state::AppState;

/// Map a resolved [`KeybindingAction`] to the TUI-side command.
///
/// `None` means no handler is wired (either intentionally — the
/// action represents a feature coco-rs hasn't built yet — or because
/// the action is layered above this dispatch point, e.g.
/// `command:foo` slash commands flow through the slash-command
/// runner, not this map).
///
/// The legacy cascade in `keybinding_bridge::map_key` only runs when
/// the resolver returned `NoMatch`; if we return `None` here the
/// keystroke is swallowed deliberately so a user-customized binding
/// doesn't accidentally fire a TUI-cascade fallback.
pub fn dispatch_action(action: &KeybindingAction, state: &AppState) -> Option<TuiCommand> {
    use KeybindingAction::*;
    Some(match action {
        // ── App-level (Global) ──────────────────────────────────────
        // Ctrl+C and Ctrl+D both go through `update::exit`'s
        // double-press machine — they do NOT immediately quit. See
        // `defaults.rs:68-71` for the TS-mirrored comment and
        // `reserved.rs` for the user-rebind block.
        AppInterrupt => TuiCommand::Interrupt,
        AppExit => TuiCommand::RequestExit,
        AppRedraw => TuiCommand::ClearScreen,
        // TS `app:toggleTodos` (Ctrl+T) — cycle the right-rail
        // expanded view between None / Tasks / (Teammates if running).
        // `update::handle_command` does the cycle math.
        AppToggleTodos => TuiCommand::ToggleExpandedTasksView,
        // TS `app:toggleTranscript` (Ctrl+O) — open the verbose,
        // scrollable transcript overlay. Pressing it again from inside
        // the overlay closes it (handled in the overlay branch below).
        AppToggleTranscript => TuiCommand::ToggleTranscript,
        // TS `app:toggleTeammatePreview` (Ctrl+Shift+O) — toggle
        // teammate spinner-line message previews on/off.
        AppToggleTeammatePreview => TuiCommand::ToggleTeammateMessagePreview,
        AppGlobalSearch => TuiCommand::ShowGlobalSearch,
        AppQuickOpen => TuiCommand::ShowQuickOpen,
        // KAIROS (`app:toggleBrief`) / TERMINAL_PANEL (`app:toggleTerminal`)
        // are TS feature-gated. coco-rs doesn't ship those features and
        // doesn't emit them in defaults; if a user explicitly binds the
        // action we silently no-op (matches TS where `useKeybinding`
        // is never registered when the feature is off).
        AppToggleBrief | AppToggleTerminal => return None,

        // ── History navigation ──────────────────────────────────────
        HistorySearch => TuiCommand::ShowCommandPalette,
        HistoryPrevious => TuiCommand::CursorUp,
        HistoryNext => TuiCommand::CursorDown,

        // ── Chat input ──────────────────────────────────────────────
        ChatCancel => {
            // Esc → Cancel always; the *second* Esc within
            // `DOUBLE_PRESS_TIMEOUT` (and only with no overlay + empty
            // input + history) opens the rewind picker. The poll is
            // here because dispatch reads + mutates the tracker
            // atomically — putting the arm in `app.rs` ahead of
            // dispatch would create the same "set then compare to
            // self" bug the old `last_esc_time` path had.
            //
            // The tracker is mutated through &AppState — see
            // `keybinding_resolver` for why `kb_handle` is `Arc<RwLock>`.
            // We don't have interior mutability for the tracker, so
            // double-press for Esc lives in `update::handle_command`'s
            // `TuiCommand::Cancel` arm via `state.ui.esc_tracker.poll`.
            TuiCommand::Cancel
        }
        ChatKillAgents => TuiCommand::KillAllAgents,
        ChatCycleMode => TuiCommand::CyclePermissionMode,
        ChatModelPicker => TuiCommand::CycleModel,
        ChatFastMode => TuiCommand::ToggleFastMode,
        ChatThinkingToggle => TuiCommand::ToggleThinking,
        // coco-rs extension: cycle Main role thinking effort through
        // the active model's `supported_thinking_levels`. See
        // `update.rs::CycleThinkingLevel`.
        ChatCycleThinking => TuiCommand::CycleThinkingLevel,
        ChatSubmit => {
            // Streaming → queue; otherwise submit (mirrors
            // keybinding_bridge.rs:259).
            if state.is_streaming() {
                TuiCommand::QueueInput
            } else {
                TuiCommand::SubmitInput
            }
        }
        ChatNewline => TuiCommand::InsertNewline,
        ChatExternalEditor => TuiCommand::OpenExternalEditor,
        // TS `chat:stash` saves the current input draft for later.
        // coco-rs implements a single-slot swap variant: pressing the
        // binding stashes the current text and restores the prior
        // stash if any — same key triggers both directions, so users
        // recover their draft with the same shortcut they used to
        // stash it. Update handler in `update.rs` does the swap.
        ChatStash => TuiCommand::StashInputDraft,
        ChatImagePaste => TuiCommand::PasteFromClipboard,
        // `chat:undo` is full input-history undo in TS
        // (`PromptInput.tsx::handleUndo` over a useUndoableState hook).
        // coco-rs hasn't ported the undoable-input stack yet; silently
        // no-op so a user-bound key doesn't fall through to the legacy
        // cascade. Implement when the stack lands.
        ChatUndo => return None,
        // `chat:messageActions` is the entry into the message-actions
        // cursor (Shift+↑ in TS). Gated on TS `MESSAGE_ACTIONS` feature;
        // coco-rs doesn't ship that overlay so we silently no-op.
        ChatMessageActions => return None,

        // ── Autocomplete ────────────────────────────────────────────
        AutocompleteAccept => TuiCommand::OverlayConfirm,
        AutocompleteDismiss => TuiCommand::Cancel,
        AutocompletePrevious => TuiCommand::OverlayPrev,
        AutocompleteNext => TuiCommand::OverlayNext,

        // ── Confirmation ────────────────────────────────────────────
        ConfirmYes => TuiCommand::Approve,
        ConfirmNo => TuiCommand::Deny,
        ConfirmPrevious => TuiCommand::OverlayPrev,
        ConfirmNext => TuiCommand::OverlayNext,
        ConfirmNextField => TuiCommand::OverlayNext,
        ConfirmPreviousField => TuiCommand::OverlayPrev,
        ConfirmCycleMode => TuiCommand::CyclePermissionMode,
        ConfirmToggle => TuiCommand::OverlayConfirm,
        ConfirmToggleExplanation => TuiCommand::ToggleSystemReminders,
        // TS dev-only debug toggle (`PermissionToggleDebug`); coco-rs
        // has no equivalent debug surface.
        PermissionToggleDebug => return None,

        // ── Tabs ────────────────────────────────────────────────────
        TabsNext => TuiCommand::SettingsNextTab,
        TabsPrevious => TuiCommand::SettingsPrevTab,

        // ── Transcript ──────────────────────────────────────────────
        // TS `transcript:toggleShowAll` flips `showAllInTranscript`
        // when the transcript screen is mounted. The handler in
        // `update::transcript` no-ops when no transcript overlay is
        // active so the keystroke is harmlessly swallowed.
        TranscriptToggleShowAll => TuiCommand::ToggleTranscriptShowAll,
        TranscriptExit => TuiCommand::Cancel,

        // ── Help ────────────────────────────────────────────────────
        HelpDismiss => TuiCommand::Cancel,

        // ── HistorySearch ───────────────────────────────────────────
        HistorySearchNext => TuiCommand::OverlayNext,
        HistorySearchAccept | HistorySearchExecute => TuiCommand::OverlayConfirm,
        HistorySearchCancel => TuiCommand::Cancel,

        // ── Task ────────────────────────────────────────────────────
        TaskBackground => TuiCommand::BackgroundAllTasks,

        // ── ThemePicker ─────────────────────────────────────────────
        // TS toggle for the syntax-highlighting setting inside the
        // theme picker overlay; coco-rs doesn't expose the option.
        ThemeToggleSyntaxHighlighting => return None,

        // ── Attachments ─────────────────────────────────────────────
        AttachmentsNext => TuiCommand::OverlayNext,
        AttachmentsPrevious => TuiCommand::OverlayPrev,
        AttachmentsExit => TuiCommand::Cancel,
        // No remove-attachment surface yet; user-bound keys silently
        // no-op until the attachments panel lands.
        AttachmentsRemove => return None,

        // ── Footer ──────────────────────────────────────────────────
        FooterUp => TuiCommand::OverlayPrev,
        FooterDown => TuiCommand::OverlayNext,
        FooterNext => TuiCommand::OverlayNext,
        FooterPrevious => TuiCommand::OverlayPrev,
        FooterOpenSelected => TuiCommand::OverlayConfirm,
        FooterClearSelection | FooterClose => TuiCommand::Cancel,

        // ── MessageSelector ─────────────────────────────────────────
        MessageSelectorUp => TuiCommand::OverlayPrev,
        MessageSelectorDown => TuiCommand::OverlayNext,
        MessageSelectorTop => TuiCommand::OverlayJumpStart,
        MessageSelectorBottom => TuiCommand::OverlayJumpEnd,
        MessageSelectorSelect => TuiCommand::OverlayConfirm,

        // ── Diff ────────────────────────────────────────────────────
        DiffDismiss => TuiCommand::Cancel,
        DiffPreviousFile => TuiCommand::OverlayPrev,
        DiffNextFile => TuiCommand::OverlayNext,
        DiffPreviousSource => TuiCommand::OverlayPrev,
        DiffNextSource => TuiCommand::OverlayNext,
        DiffBack => TuiCommand::Cancel,
        DiffViewDetails => TuiCommand::OverlayConfirm,

        // ── ModelPicker ─────────────────────────────────────────────
        // Left/Right cycle the *effort axis* — separate from Up/Down
        // (`SelectPrevious` / `SelectNext`) which move between models.
        // Previously both pairs routed to OverlayPrev/OverlayNext, so
        // ←/→ silently scrolled the list (latent TS-parity gap).
        ModelPickerDecreaseEffort => TuiCommand::ModelPickerCycleEffort(-1),
        ModelPickerIncreaseEffort => TuiCommand::ModelPickerCycleEffort(1),

        // ── Select ──────────────────────────────────────────────────
        SelectNext => TuiCommand::OverlayNext,
        SelectPrevious => TuiCommand::OverlayPrev,
        SelectAccept => TuiCommand::OverlayConfirm,
        SelectCancel => TuiCommand::Cancel,

        // ── Plugin ──────────────────────────────────────────────────
        // Plugin overlay actions (`space` toggle / `i` install) bound
        // in the `Plugin` context. coco-rs doesn't open a Plugin
        // overlay so the context never activates from defaults; if a
        // user re-binds one of these to a global context we silently
        // no-op until the overlay lands.
        PluginToggle | PluginInstall => return None,

        // ── Settings ────────────────────────────────────────────────
        SettingsClose => TuiCommand::OverlayConfirm,
        // SettingsSearch / SettingsRetry are inside-overlay state
        // machine actions (not application-level TuiCommands). The
        // Settings overlay reads them directly from the resolver
        // when it owns key dispatch — they intentionally route through
        // None here. Once the overlay state machine ports them,
        // promote to actual TuiCommand variants.
        SettingsSearch | SettingsRetry => return None,

        // ── Voice ───────────────────────────────────────────────────
        // TS `VOICE_MODE` feature gate; coco-rs has no voice subsystem.
        VoicePushToTalk => return None,

        // ── Scroll (internal) ───────────────────────────────────────
        ScrollPageUp => TuiCommand::PageUp,
        ScrollPageDown => TuiCommand::PageDown,
        ScrollLineUp => TuiCommand::ScrollUp,
        ScrollLineDown => TuiCommand::ScrollDown,
        ScrollTop => TuiCommand::OverlayJumpStart,
        ScrollBottom => TuiCommand::OverlayJumpEnd,

        // ── Selection ───────────────────────────────────────────────
        SelectionCopy => TuiCommand::CopyLastMessage,

        // ── Slash command escape hatch ──────────────────────────────
        // `command:foo` user binding from `keybindings.json` →
        // synthesize a `/foo` submit. The agent driver's existing
        // slash-command runner handles the dispatch.
        Command(name) => TuiCommand::ExecuteSlashCommand(name.clone()),

        // ── MessageActions:* (11 variants) ───────────────────────────
        // Internal context; the validator rejects user bindings into it,
        // so these only fire from defaults — and `MESSAGE_ACTIONS` isn't
        // ported, so no defaults emit them. Match arm exists purely so
        // the match is exhaustive without a wildcard. Returning None
        // matches TS, where the cursor handlers are only registered
        // while the message-actions overlay is mounted.
        MessageActionsPrev
        | MessageActionsNext
        | MessageActionsTop
        | MessageActionsBottom
        | MessageActionsPrevUser
        | MessageActionsNextUser
        | MessageActionsEscape
        | MessageActionsCtrlC
        | MessageActionsEnter
        | MessageActionsC
        | MessageActionsP => return None,
    })
}

#[cfg(test)]
#[path = "keybinding_dispatch.test.rs"]
mod tests;
