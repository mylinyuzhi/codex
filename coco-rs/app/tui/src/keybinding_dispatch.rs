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
use crate::state::SlashCommandName;

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
        // scrollable transcript state. Pressing it again from inside
        // the state closes it (handled in the state branch below).
        AppToggleTranscript => TuiCommand::ToggleTranscript,
        // TS `app:toggleTeammatePreview` (Ctrl+Shift+O) — toggle
        // teammate spinner-line message previews on/off.
        AppToggleTeammatePreview => TuiCommand::ToggleTeammateMessagePreview,
        // coco-rs-only `app:toggleTeamRoster` (Ctrl+Shift+T) — open the
        // teammate roster / mode picker. Gated on the session having a
        // teammate so the binding is an inert no-op in non-team sessions
        // (it returns `None`, swallowing the key without a cascade fallthrough)
        // rather than shadowing the key globally.
        AppToggleTeamRoster => {
            return state
                .session
                .subagents
                .iter()
                .any(|s| s.kind == crate::state::SubagentKind::Teammate)
                .then_some(TuiCommand::OpenTeamRoster);
        }
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
            // `DOUBLE_PRESS_TIMEOUT` (and only with no state + empty
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
            // SubmitInput owns the streaming decision: it queues by default
            // and emits submit_interrupt only when every running tool is
            // cancel-interruptible.
            TuiCommand::SubmitInput
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
        // coco-rs doesn't ship that state so we silently no-op.
        ChatMessageActions => return None,

        // ── Autocomplete ────────────────────────────────────────────
        AutocompleteAccept => TuiCommand::AutocompleteAccept,
        AutocompleteDismiss => TuiCommand::Cancel,
        AutocompletePrevious => TuiCommand::SurfacePrev,
        AutocompleteNext => TuiCommand::SurfaceNext,

        // ── Confirmation ────────────────────────────────────────────
        ConfirmYes => TuiCommand::Approve,
        ConfirmNo => TuiCommand::Deny,
        ConfirmPrevious => TuiCommand::SurfacePrev,
        ConfirmNext => TuiCommand::SurfaceNext,
        ConfirmNextField => TuiCommand::SurfaceNext,
        ConfirmPreviousField => TuiCommand::SurfacePrev,
        ConfirmCycleMode => TuiCommand::CyclePermissionMode,
        ConfirmToggle => TuiCommand::SurfaceConfirm,
        ConfirmToggleExplanation => TuiCommand::TogglePermissionExplanation,
        // TS dev-only debug toggle (`PermissionToggleDebug`); coco-rs
        // has no equivalent debug surface.
        PermissionToggleDebug => return None,

        // ── Tabs ────────────────────────────────────────────────────
        TabsNext => TuiCommand::SettingsNextTab,
        TabsPrevious => TuiCommand::SettingsPrevTab,

        // ── Transcript ──────────────────────────────────────────────
        TranscriptExit => TuiCommand::Cancel,

        // ── Help ────────────────────────────────────────────────────
        HelpDismiss => TuiCommand::Cancel,

        // ── HistorySearch ───────────────────────────────────────────
        HistorySearchNext => TuiCommand::SurfaceNext,
        HistorySearchAccept | HistorySearchExecute => TuiCommand::SurfaceConfirm,
        HistorySearchCancel => TuiCommand::Cancel,

        // ── Task ────────────────────────────────────────────────────
        TaskBackground => TuiCommand::BackgroundAllTasks,

        // ── ThemePicker ─────────────────────────────────────────────
        ThemeToggleSyntaxHighlighting => TuiCommand::ToggleSyntaxHighlighting,

        // ── Attachments ─────────────────────────────────────────────
        AttachmentsNext => TuiCommand::SurfaceNext,
        AttachmentsPrevious => TuiCommand::SurfacePrev,
        AttachmentsExit => TuiCommand::Cancel,
        // No remove-attachment surface yet; user-bound keys silently
        // no-op until the attachments panel lands.
        AttachmentsRemove => return None,

        // ── Footer ──────────────────────────────────────────────────
        FooterUp => TuiCommand::SurfacePrev,
        FooterDown => TuiCommand::SurfaceNext,
        FooterNext => TuiCommand::SurfaceNext,
        FooterPrevious => TuiCommand::SurfacePrev,
        FooterOpenSelected => TuiCommand::SurfaceConfirm,
        FooterClearSelection | FooterClose => TuiCommand::Cancel,

        // ── MessageSelector ─────────────────────────────────────────
        MessageSelectorUp => TuiCommand::SurfacePrev,
        MessageSelectorDown => TuiCommand::SurfaceNext,
        MessageSelectorTop => TuiCommand::SurfaceJumpStart,
        MessageSelectorBottom => TuiCommand::SurfaceJumpEnd,
        MessageSelectorSelect => TuiCommand::SurfaceConfirm,

        // ── Diff ────────────────────────────────────────────────────
        DiffDismiss => TuiCommand::Cancel,
        DiffPreviousFile => TuiCommand::SurfacePrev,
        DiffNextFile => TuiCommand::SurfaceNext,
        DiffPreviousSource => TuiCommand::SurfacePrev,
        DiffNextSource => TuiCommand::SurfaceNext,
        DiffBack => TuiCommand::Cancel,
        DiffViewDetails => TuiCommand::SurfaceConfirm,

        // ── ModelPicker ─────────────────────────────────────────────
        // Left/Right cycle the *effort axis* — separate from Up/Down
        // (`SelectPrevious` / `SelectNext`) which move between models.
        // Previously both pairs routed to SurfacePrev/SurfaceNext, so
        // ←/→ silently scrolled the list (latent TS-parity gap).
        ModelPickerDecreaseEffort => TuiCommand::ModelPickerCycleEffort(-1),
        ModelPickerIncreaseEffort => TuiCommand::ModelPickerCycleEffort(1),

        // ── Select ──────────────────────────────────────────────────
        SelectNext => TuiCommand::SurfaceNext,
        SelectPrevious => TuiCommand::SurfacePrev,
        SelectAccept => TuiCommand::SurfaceConfirm,
        SelectCancel => TuiCommand::Cancel,

        // ── Plugin ──────────────────────────────────────────────────
        // Plugin state actions (`space` toggle / `i` install) bound
        // in the `Plugin` context. coco-rs doesn't open a Plugin
        // state so the context never activates from defaults; if a
        // user re-binds one of these to a global context we silently
        // no-op until the state lands.
        PluginToggle | PluginInstall => return None,

        // ── Settings ────────────────────────────────────────────────
        SettingsClose => TuiCommand::SurfaceConfirm,
        // SettingsSearch / SettingsRetry are inside-state state
        // machine actions (not application-level TuiCommands). The
        // Settings state reads them directly from the resolver
        // when it owns key dispatch — they intentionally route through
        // None here. Once the state state machine ports them,
        // promote to actual TuiCommand variants.
        SettingsSearch | SettingsRetry => return None,

        // ── Voice ───────────────────────────────────────────────────
        // TS `VOICE_MODE` feature gate; coco-rs has no voice subsystem.
        VoicePushToTalk => return None,

        // ── Scroll (internal) ───────────────────────────────────────
        ScrollPageUp if transcript_active(state) => TuiCommand::TranscriptPage(-1),
        ScrollPageUp => TuiCommand::PageUp,
        ScrollPageDown if transcript_active(state) => TuiCommand::TranscriptPage(1),
        ScrollPageDown => TuiCommand::PageDown,
        ScrollLineUp if transcript_active(state) => TuiCommand::TranscriptScrollLines(-1),
        ScrollLineUp => TuiCommand::ScrollUp,
        ScrollLineDown if transcript_active(state) => TuiCommand::TranscriptScrollLines(1),
        ScrollLineDown => TuiCommand::ScrollDown,
        ScrollTop if transcript_active(state) => TuiCommand::TranscriptJumpStart,
        ScrollTop => TuiCommand::SurfaceJumpStart,
        ScrollBottom if transcript_active(state) => TuiCommand::TranscriptJumpEnd,
        ScrollBottom => TuiCommand::SurfaceJumpEnd,

        // ── Selection ───────────────────────────────────────────────
        SelectionCopy => TuiCommand::CopyLastMessage,

        // ── Slash command escape hatch ──────────────────────────────
        // `command:foo` user binding from `keybindings.json` →
        // synthesize a `/foo` submit. The agent driver's existing
        // slash-command runner handles the dispatch.
        Command(name) => SlashCommandName::new(name.clone())
            .map(TuiCommand::ExecuteSlashCommand)
            .unwrap_or(TuiCommand::Noop),

        // ── MessageActions:* (11 variants) ───────────────────────────
        // Internal context; the validator rejects user bindings into it,
        // so these only fire from defaults — and `MESSAGE_ACTIONS` isn't
        // ported, so no defaults emit them. Match arm exists purely so
        // the match is exhaustive without a wildcard. Returning None
        // matches TS, where the cursor handlers are only registered
        // while the message-actions state is mounted.
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

fn transcript_active(state: &AppState) -> bool {
    matches!(
        state.ui.modal,
        Some(crate::state::ModalState::Transcript(_))
    )
}

#[cfg(test)]
#[path = "keybinding_dispatch.test.rs"]
mod tests;
