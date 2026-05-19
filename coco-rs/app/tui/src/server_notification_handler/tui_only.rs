//! TUI-only handler.
//!
//! Handles [`TuiOnlyEvent`] — pane prompts, modals, picker data-ready
//! signals, compaction/speculation/cron toasts, and TUI-specific rewind
//! metadata. SDK and bridge consumers drop these events; only the TUI acts
//! on them.
//!
//! Complex event-specific logic (`DiffStatsReady`, `RewindCompleted`) is
//! extracted into named helpers — `on_diff_stats_loaded`,
//! `on_rewind_completed` — so the match arms stay scannable.

use coco_messages::SystemMessageLevel;
use coco_types::TuiOnlyEvent;

use crate::command::SystemPushKind;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::ModalState;
use crate::state::PanePromptState;
use crate::state::ui::Toast;

/// Queue a TUI-originated system message for engine round-trip.
/// The App loop drains `state.session.pending_system_pushes` after
/// each `handle_core_event` and dispatches `UserCommand::PushSystemMessage`.
fn enqueue_informational(
    state: &mut AppState,
    level: SystemMessageLevel,
    title: &str,
    message: String,
) {
    state
        .session
        .pending_system_pushes
        .push_back(SystemPushKind::Informational {
            level,
            title: title.to_string(),
            message,
        });
}

#[cfg(test)]
#[path = "tui_only.test.rs"]
mod tests;

pub(super) fn handle(state: &mut AppState, event: TuiOnlyEvent) -> bool {
    match event {
        TuiOnlyEvent::ApprovalRequired {
            request_id,
            tool_name,
            description,
            display_input,
            show_always_allow,
            choices,
            permission_suggestions,
            original_input,
        } => {
            state.ui.push_delayed_permission(
                crate::state::PermissionPromptState {
                    request_id,
                    tool_name,
                    description,
                    detail: crate::state::PermissionDetail::Generic {
                        input_preview: display_input.as_display_str().to_string(),
                    },
                    risk_level: None,
                    // When the dialog is in choice mode, suppress the
                    // "Always Allow" affordance — the user is picking
                    // a one-shot action (e.g. keep/clear context), not
                    // declaring a durable rule.
                    show_always_allow: choices.is_none() && show_always_allow,
                    classifier_checking: false,
                    classifier_auto_approved: None,
                    choices,
                    selected_choice: 0,
                    display_input,
                    original_input,
                    permission_suggestions,
                },
                std::time::Instant::now(),
            );
            true
        }
        TuiOnlyEvent::DiffStatsReady {
            message_id,
            files_changed,
            insertions,
            deletions,
            file_paths,
        } => on_diff_stats_loaded(
            state,
            message_id,
            files_changed,
            insertions,
            deletions,
            file_paths,
        ),
        TuiOnlyEvent::RewindCompleted {
            target_message_id,
            files_changed,
        } => on_rewind_completed(state, target_message_id, files_changed),

        // === Question / elicitation / sandbox prompts ===
        TuiOnlyEvent::QuestionAsked { request_id, input } => {
            let questions = parse_question_items(&input);
            // Plan-mode gate for the Skip-interview footer item — TS:
            // `isInPlanMode` from `getPermissionMode()` at
            // `AskUserQuestionPermissionRequest.tsx`. Captured at state
            // construction so a mid-state mode flip doesn't change the
            // available footer items mid-flight.
            let is_in_plan_mode = state.session.permission_mode == coco_types::PermissionMode::Plan;
            state.ui.push_prompt(PanePromptState::Question(
                crate::state::QuestionPromptState {
                    request_id,
                    original_input: input,
                    questions,
                    focus: crate::state::QuestionFocus::Question(0),
                    is_in_plan_mode,
                },
            ));
            true
        }
        TuiOnlyEvent::ElicitationRequested {
            request_id, server, ..
        } => {
            tracing::warn!(
                %request_id,
                %server,
                "dropping unsupported TUI elicitation request"
            );
            state.ui.add_toast(Toast::error(
                t!("toast.elicitation_unsupported", server = server.as_str()).to_string(),
            ));
            true
        }
        TuiOnlyEvent::SandboxApprovalRequired {
            request_id,
            operation,
        } => {
            state.ui.push_prompt(PanePromptState::SandboxPermission(
                crate::state::SandboxPermissionPromptState {
                    request_id,
                    description: operation,
                },
            ));
            true
        }

        // === Picker data-ready ===
        TuiOnlyEvent::PluginDataReady { plugins } => {
            state.session.available_plugins = plugins;
            true
        }
        TuiOnlyEvent::OutputStylesReady { styles } => {
            state.session.available_output_styles = styles;
            true
        }
        TuiOnlyEvent::AvailableCommandsRefreshed { commands } => {
            // Overwrite, not extend — the producer always sends the
            // full visible set so this trivially handles command
            // removals (e.g. plugin uninstall via /reload-plugins).
            state.session.available_commands = commands;
            // If the user is mid-`/` query, recompute the popup
            // against the new list so the next render reflects it
            // without waiting for another keystroke.
            crate::autocomplete::refresh_suggestions(state);
            true
        }
        // No-op: checkpoint data consumed by ShowRewind state, not stored.
        TuiOnlyEvent::RewindCheckpointsReady { .. } => false,

        // === Compaction / speculation toasts ===
        TuiOnlyEvent::CompactionCircuitBreakerOpen { failures } => {
            state.ui.add_toast(Toast::warning(
                t!("toast.compaction_breaker", failures = failures).to_string(),
            ));
            true
        }
        TuiOnlyEvent::MicroCompactionApplied { removed } => {
            state.ui.add_toast(Toast::info(
                t!("toast.micro_compaction", removed = removed).to_string(),
            ));
            true
        }
        TuiOnlyEvent::SessionMemoryCompactApplied { summary_tokens } => {
            state.ui.add_toast(Toast::info(
                t!("toast.session_memory_compacted", tokens = summary_tokens).to_string(),
            ));
            true
        }
        TuiOnlyEvent::SpeculativeRolledBack { reason } => {
            state.ui.add_toast(Toast::warning(
                t!("toast.speculation_rolled_back", reason = reason.as_str()).to_string(),
            ));
            true
        }

        // === Memory extraction toasts ===
        TuiOnlyEvent::SessionMemoryExtractionStarted => {
            state
                .ui
                .add_toast(Toast::info(t!("toast.extracting_memories").to_string()));
            true
        }
        TuiOnlyEvent::SessionMemoryExtractionCompleted { extracted } => {
            state.ui.add_toast(Toast::success(
                t!("toast.memories_extracted_count", count = extracted).to_string(),
            ));
            true
        }
        TuiOnlyEvent::SessionMemoryExtractionFailed { error } => {
            state.ui.add_toast(Toast::error(
                t!("toast.memory_extract_failed_full", error = error.as_str()).to_string(),
            ));
            true
        }

        // === Cron toasts ===
        TuiOnlyEvent::CronJobDisabled { job_id: _, reason } => {
            state.ui.add_toast(Toast::warning(
                t!("toast.cron_disabled_full", reason = reason.as_str()).to_string(),
            ));
            true
        }
        TuiOnlyEvent::CronJobsMissed { count } => {
            state.ui.add_toast(Toast::warning(
                t!("toast.cron_missed_count", count = count).to_string(),
            ));
            true
        }

        // === Streaming tool display ===
        TuiOnlyEvent::ToolCallDelta { call_id, delta } => {
            let Some(tool) = state
                .session
                .tool_executions
                .iter_mut()
                .find(|t| t.call_id == call_id)
            else {
                return false;
            };
            tool.streaming_input
                .get_or_insert_with(String::new)
                .push_str(&delta);
            true
        }
        TuiOnlyEvent::ToolProgress { tool_use_id, data } => {
            let Some(tool) = state
                .session
                .tool_executions
                .iter_mut()
                .find(|t| t.call_id == tool_use_id)
            else {
                return false;
            };
            if let Some(desc) = data.get("description").and_then(|d| d.as_str()) {
                tool.description = Some(desc.to_string());
            }
            true
        }
        TuiOnlyEvent::ToolExecutionAborted {
            tool_use_id: _,
            reason,
        } => {
            state.ui.add_toast(Toast::warning(
                t!("toast.tool_aborted_full", reason = reason.as_str()).to_string(),
            ));
            true
        }

        // === Slash-command result (System role transcript line) ===
        TuiOnlyEvent::SlashCommandResult { name: _, text } => {
            // Round-trip through engine `MessageHistory` so the transcript
            // view (and SDK consumers / JSONL transcript) see the slash
            // output via the standard `MessageAppended` path. The
            // renderer treats empty-title `Informational` as a bare
            // SystemText body.
            enqueue_informational(state, SystemMessageLevel::Info, "", text);
            true
        }
        // === Open the rewind picker state ===
        // The command layer lacks direct AppState access, so it routes
        // through this event.
        TuiOnlyEvent::OpenRewindPicker => {
            let rewind = crate::update_rewind::build_rewind_state(state);
            if rewind.messages.is_empty() {
                state
                    .ui
                    .add_toast(Toast::info(t!("toast.no_rewind_messages").to_string()));
            } else {
                state.ui.show_modal(ModalState::Rewind(rewind));
            }
            true
        }
        // === Open the /memory file picker state ===
        // Entries are pre-built by the slash dispatcher (no extra state
        // lookup needed here). On select the TUI sends a command to the
        // CLI bridge; on cancel it emits a transcript line + toast.
        // TS: `commands/memory/memory.tsx`'s pre-flight render.
        TuiOnlyEvent::OpenMemoryDialog { entries } => {
            if entries.is_empty() {
                state
                    .ui
                    .add_toast(Toast::warning(t!("dialog.memory_no_files").to_string()));
            } else {
                state.ui.show_modal(ModalState::MemoryDialog(
                    crate::state::MemoryDialogState::from_wire(entries),
                ));
            }
            true
        }
        TuiOnlyEvent::MemoryFileOpened { path } => {
            let text = t!("toast.memory_opened", path = path.as_str()).to_string();
            enqueue_informational(state, SystemMessageLevel::Info, "", text.clone());
            state.ui.add_toast(Toast::info(text));
            true
        }
        TuiOnlyEvent::MemoryFileOpenFailed { path: _, error } => {
            let text = t!("toast.memory_open_failed", error = error.as_str()).to_string();
            enqueue_informational(state, SystemMessageLevel::Warning, "", text.clone());
            state.ui.add_toast(Toast::warning(text));
            true
        }
        TuiOnlyEvent::PlanFileOpened { path } => {
            let text = t!("toast.plan_opened", path = path.as_str()).to_string();
            enqueue_informational(state, SystemMessageLevel::Info, "", text.clone());
            state.ui.add_toast(Toast::info(text));
            true
        }
        TuiOnlyEvent::PlanFileOpenFailed { path: _, error } => {
            let text = t!("toast.plan_open_failed", error = error.as_str()).to_string();
            enqueue_informational(state, SystemMessageLevel::Warning, "", text.clone());
            state.ui.add_toast(Toast::warning(text));
            true
        }
        TuiOnlyEvent::ExternalEditorPrepare { .. } => false,
        TuiOnlyEvent::PromptEditorCompleted { content, modified } => {
            state.ui.input.set_text(&content);
            state.ui.input.textarea.set_cursor(content.len());
            let text = if modified {
                t!("toast.prompt_editor_updated")
            } else {
                t!("toast.prompt_editor_unchanged")
            }
            .to_string();
            state.ui.add_toast(Toast::info(text));
            true
        }
        TuiOnlyEvent::PromptEditorFailed { error } => {
            state.ui.add_toast(Toast::warning(
                t!("toast.prompt_editor_failed", error = error.as_str()).to_string(),
            ));
            true
        }
        TuiOnlyEvent::BashCommandCompleted {
            user_message_id: _,
            output: _,
            exit_code: _,
        } => {
            // Visible bash output flows through the standard engine
            // path: `run_prompt_mode_bash` pushes a single
            // `SystemMessage::LocalCommand { command, output }` via
            // `history_push_and_emit` before this event fires, so the
            // transcript view already shows the result by the time we
            // reach here. The event is kept for observability (the
            // `emit.rs` event-name map still surfaces it on the wire)
            // but no longer drives TUI transcript writes.
            false
        }
        TuiOnlyEvent::OpenModelPicker => {
            crate::update::show::cycle_model(state);
            true
        }
        TuiOnlyEvent::SlashCommandStatus { name, kind } => {
            use coco_types::SlashCommandStatusKind;
            let (level, text) = match kind {
                SlashCommandStatusKind::NoHandler => (
                    SystemMessageLevel::Warning,
                    t!("slash.status.no_handler", name = name.as_str()).to_string(),
                ),
                SlashCommandStatusKind::Failed { error } => (
                    SystemMessageLevel::Error,
                    t!(
                        "slash.status.failed",
                        name = name.as_str(),
                        error = error.as_str()
                    )
                    .to_string(),
                ),
                SlashCommandStatusKind::EmptyPrompt => (
                    SystemMessageLevel::Info,
                    t!("slash.status.empty_prompt", name = name.as_str()).to_string(),
                ),
                SlashCommandStatusKind::DialogPending { dialog_kind } => (
                    SystemMessageLevel::Info,
                    t!(
                        "slash.status.dialog_pending",
                        name = name.as_str(),
                        dialog_kind = dialog_kind.as_str()
                    )
                    .to_string(),
                ),
                SlashCommandStatusKind::PermissionsUsageAllow => (
                    SystemMessageLevel::Info,
                    t!("slash.permissions.usage_allow").to_string(),
                ),
                SlashCommandStatusKind::PermissionsUsageDeny => (
                    SystemMessageLevel::Info,
                    t!("slash.permissions.usage_deny").to_string(),
                ),
            };
            enqueue_informational(state, level, "", text);
            true
        }
    }
}

fn on_diff_stats_loaded(
    state: &mut AppState,
    stats_message_id: String,
    diff_files: i32,
    insertions: i64,
    deletions: i64,
    file_paths: Vec<String>,
) -> bool {
    let has_any_changes = diff_files > 0;
    let preview = crate::state::DiffStatsPreview {
        files_changed: diff_files,
        insertions,
        deletions,
        file_paths,
    };
    if let Some(ModalState::Rewind(r)) = state.ui.modal.as_mut() {
        // Per-row metadata for the pick-list. TS: `fileHistoryMetadata`
        // map keyed by item index (`MessageSelector.tsx:285-312`).
        if let Some(row) = r
            .messages
            .iter_mut()
            .find(|m| m.message_id == stats_message_id)
        {
            row.diff_stats = Some(preview.clone());
            row.can_restore_code = Some(true);
        }
        // Selected-row aggregates drive the RestoreOptions phase.
        let selected_id = r
            .messages
            .get(r.selected as usize)
            .map(|m| m.message_id.as_str());
        if selected_id == Some(&stats_message_id) {
            r.has_file_changes = has_any_changes;
            r.diff_stats = Some(preview);
            r.available_options = crate::state::rewind::build_restore_options(
                r.file_history_enabled,
                has_any_changes,
                r.allow_summarize_up_to,
            );
        }
    }
    true
}

fn on_rewind_completed(
    state: &mut AppState,
    target_message_id: String,
    files_changed: i32,
) -> bool {
    let mut restored_permission_mode = None;
    let mut restored_input_text = None;

    let mut restored_image_path: Option<String> = None;
    if !target_message_id.is_empty() {
        // Search the engine-authoritative cell list for the rewound
        // message. UI restoration reads `cell.source` directly.
        let cells = state.session.transcript.cells();
        if let Some(target_cell) = cells
            .iter()
            .find(|c| c.message_uuid.to_string() == target_message_id)
        {
            if let coco_messages::Message::User(u) = target_cell.source.as_ref() {
                restored_permission_mode = u.permission_mode;
            }
            // TS `textForResubmit` (`utils/messages.ts:2873-2886`) strips
            // IDE-injected context tags so the restored prompt doesn't
            // leak `<ide_opened_file>` / `<ide_selection>` blocks.
            let raw = match &target_cell.kind {
                crate::state::transcript_view::CellKind::UserText { text } => text.as_str(),
                _ => "",
            };
            let stripped = crate::update_rewind::strip_ide_context_tags(raw);
            restored_input_text = Some(stripped).filter(|s| !s.is_empty());
            // TS `restoreMessageSync` (`screens/REPL.tsx:3721-3737`)
            // restores pasted images by reading them off the rewound
            // message. The image path lives on
            // `UserContentPart::File` with an `image/*` media type;
            // we surface only the first one (matches TS shape).
            if let coco_messages::Message::User(u) = target_cell.source.as_ref()
                && let coco_messages::LlmMessage::User { content, .. } = &u.message
            {
                for part in content {
                    if let coco_messages::UserContent::File(f) = part
                        && f.media_type.starts_with("image/")
                        && let Some(url) = f.data.as_url()
                    {
                        restored_image_path = Some(url.to_string());
                        break;
                    }
                }
            }
        }
    }

    // The engine emits `MessageTruncated` after this handler — that
    // event truncates `state.session.transcript`, the single source
    // of truth for rendering.

    if let Some(mode) = restored_permission_mode {
        state.session.permission_mode = mode;
    }

    if let Some(text) = restored_input_text {
        state.ui.input.textarea.set_text(&text);
        let eol = state.ui.input.textarea.end_of_current_line();
        state.ui.input.textarea.set_cursor(eol);
    }

    // Rotate conversation_id on truncate so the next request breaks
    // any prior cache key. TS: setConversationId(randomUUID()) inside
    // rewindConversationTo (`screens/REPL.tsx:3673`).
    if !target_message_id.is_empty() {
        state.session.conversation_id = Some(uuid::Uuid::new_v4().to_string());
    }

    // Clear the prompt-suggestion belt — stale suggestions from
    // earlier turns are no longer valid in the rewound conversation.
    // TS: setAppState({...prev, promptSuggestion: {text: null, ...}})
    // (`screens/REPL.tsx:3699-3705`).
    state.session.prompt_suggestions.clear();

    // Paste buffer handling — TS `restoreMessageSync` rebuilds
    // `pastedContents` from the rewound message's image blocks
    // (`screens/REPL.tsx:3721-3737`). Each user message carries at most
    // one image; if present, re-attach it; otherwise clear any leftover
    // paste-buffer state so it doesn't leak into the new turn.
    state.ui.paste_manager.clear();
    if let Some(path) = restored_image_path {
        state.ui.paste_manager.add_image(path);
    }

    state.ui.scroll_offset = 0;
    state.ui.user_scrolled = false;
    state.ui.dismiss_modal();

    let msg = if files_changed > 0 {
        t!("toast.rewound_checkpoint", count = files_changed).to_string()
    } else {
        t!("toast.conversation_rewound_checkpoint").to_string()
    };
    state.ui.add_toast(Toast::success(msg));
    true
}

/// Parse the AskUserQuestion tool input dict into rich
/// `QuestionItem`s the state can render.
///
/// Tolerant parser — missing/optional fields use defaults so a
/// model that emits a partial schema still produces a usable state
/// rather than a blank screen.
///
/// TS: `AskUserQuestionPermissionRequest.tsx` reads the same shape.
fn parse_question_items(input: &serde_json::Value) -> Vec<crate::state::QuestionItem> {
    let Some(arr) = input.get("questions").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .map(|q| {
            let header = str_field(q, "header").to_string();
            let question = str_field(q, "question").to_string();
            let multi_select = q
                .get("multiSelect")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let options: Vec<crate::state::QuestionOption> = q
                .get("options")
                .and_then(serde_json::Value::as_array)
                .map(|opts| {
                    opts.iter()
                        .map(|o| crate::state::QuestionOption {
                            label: str_field(o, "label").to_string(),
                            description: str_field(o, "description").to_string(),
                            preview: o
                                .get("preview")
                                .and_then(serde_json::Value::as_str)
                                .map(String::from),
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Inject the "Other" sentinel as the last option of every
            // question (single-select only — TS does the same; the
            // multiSelect widget hides Other). When the user focuses
            // it, typed chars route to `notes` and the answer-build
            // logic substitutes the typed text for the option label.
            // TS: `QuestionView.tsx:85` `__other__` sentinel.
            let mut options = options;
            if !multi_select {
                options.push(crate::state::QuestionOption {
                    label: crate::state::OTHER_OPTION_LABEL.into(),
                    description: "Type your own answer.".into(),
                    preview: None,
                });
            }
            crate::state::QuestionItem {
                header,
                question,
                options,
                multi_select,
                selected: 0,
                checked: Vec::new(),
                notes: String::new(),
                editing_notes: false,
            }
        })
        .collect()
}

fn str_field<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(serde_json::Value::as_str).unwrap_or("")
}
