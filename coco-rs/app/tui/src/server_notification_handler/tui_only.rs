//! TUI-only handler.
//!
//! Handles [`TuiOnlyEvent`] — overlays (permission, question, elicitation,
//! sandbox), picker data-ready signals, compaction/speculation/cron toasts,
//! and TUI-specific rewind metadata. SDK and bridge consumers drop these
//! events; only the TUI acts on them.
//!
//! Complex event-specific logic (`DiffStatsReady`, `RewindCompleted`) is
//! extracted into named helpers — `on_diff_stats_loaded`,
//! `on_rewind_completed` — so the match arms stay scannable.

use coco_types::TuiOnlyEvent;

use crate::i18n::t;
use crate::state::AppState;
use crate::state::ui::Toast;

pub(super) fn handle(state: &mut AppState, event: TuiOnlyEvent) -> bool {
    match event {
        TuiOnlyEvent::ApprovalRequired {
            request_id,
            tool_name,
            description,
            input_preview,
        } => {
            state.ui.set_overlay(crate::state::Overlay::Permission(
                crate::state::PermissionOverlay {
                    request_id,
                    tool_name,
                    description,
                    detail: crate::state::PermissionDetail::Generic { input_preview },
                    risk_level: None,
                    show_always_allow: true,
                    classifier_checking: false,
                    classifier_auto_approved: None,
                },
            ));
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

        // === Question / elicitation / sandbox overlays ===
        TuiOnlyEvent::QuestionAsked { request_id, input } => {
            let questions = parse_question_items(&input);
            // Plan-mode gate for the Skip-interview footer item — TS:
            // `isInPlanMode` from `getPermissionMode()` at
            // `AskUserQuestionPermissionRequest.tsx`. Captured at overlay
            // construction so a mid-overlay mode flip doesn't change the
            // available footer items mid-flight.
            let is_in_plan_mode = state.session.permission_mode == coco_types::PermissionMode::Plan;
            state.ui.set_overlay(crate::state::Overlay::Question(
                crate::state::QuestionOverlay {
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
            request_id,
            server,
            schema,
        } => {
            let fields = parse_elicitation_fields(&schema);
            state.ui.set_overlay(crate::state::Overlay::Elicitation(
                crate::state::ElicitationOverlay {
                    request_id,
                    server_name: server,
                    message: String::new(),
                    fields,
                },
            ));
            true
        }
        TuiOnlyEvent::SandboxApprovalRequired {
            request_id,
            operation,
        } => {
            state
                .ui
                .set_overlay(crate::state::Overlay::SandboxPermission(
                    crate::state::SandboxPermissionOverlay {
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
        // No-op: checkpoint data consumed by ShowRewind overlay, not stored.
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
            // `system_text` produces `MessageContent::SystemText`, which the
            // chat widget routes to `render_system` (system styling, no `>`
            // user-prefix). `is_meta` defaults to false so the body shows
            // expanded — slash-command output is the answer the user asked
            // for, not a collapsible reminder.
            state
                .session
                .add_message(crate::state::session::ChatMessage::system_text(
                    uuid::Uuid::new_v4().to_string(),
                    text,
                ));
            true
        }
        // === Open the rewind picker overlay (palette path) ===
        // Typed `/rewind` is intercepted earlier in `update/edit.rs::try_local_command`
        // and never round-trips through CoreEvent. The palette path lacks
        // direct AppState access, so we route through this event.
        TuiOnlyEvent::OpenRewindPicker => {
            let overlay = crate::update_rewind::build_rewind_overlay(state);
            if overlay.messages.is_empty() {
                state
                    .ui
                    .add_toast(Toast::info(t!("toast.no_rewind_messages").to_string()));
            } else {
                state.ui.set_overlay(crate::state::Overlay::Rewind(overlay));
            }
            true
        }
        // === Open the /memory file picker overlay ===
        // Entries are pre-built by the slash dispatcher (no extra state
        // lookup needed here). On select the TUI creates the file +
        // launches `$VISUAL || $EDITOR`; on cancel it emits a toast.
        // TS: `commands/memory/memory.tsx`'s pre-flight render.
        TuiOnlyEvent::OpenMemoryDialog { entries } => {
            if entries.is_empty() {
                state
                    .ui
                    .add_toast(Toast::warning(t!("dialog.memory_no_files").to_string()));
            } else {
                state.ui.set_overlay(crate::state::Overlay::MemoryDialog(
                    crate::state::MemoryDialogOverlay::from_wire(entries),
                ));
            }
            true
        }
        TuiOnlyEvent::SlashCommandStatus { name, kind } => {
            use coco_types::SlashCommandStatusKind;
            let text = match kind {
                SlashCommandStatusKind::NoHandler => {
                    t!("slash.status.no_handler", name = name.as_str()).to_string()
                }
                SlashCommandStatusKind::Failed { error } => t!(
                    "slash.status.failed",
                    name = name.as_str(),
                    error = error.as_str()
                )
                .to_string(),
                SlashCommandStatusKind::EmptyPrompt => {
                    t!("slash.status.empty_prompt", name = name.as_str()).to_string()
                }
                SlashCommandStatusKind::DialogPending { dialog_kind } => t!(
                    "slash.status.dialog_pending",
                    name = name.as_str(),
                    dialog_kind = dialog_kind.as_str()
                )
                .to_string(),
                SlashCommandStatusKind::PermissionsUsageAllow => {
                    t!("slash.permissions.usage_allow").to_string()
                }
                SlashCommandStatusKind::PermissionsUsageDeny => {
                    t!("slash.permissions.usage_deny").to_string()
                }
            };
            state
                .session
                .add_message(crate::state::session::ChatMessage::system_text(
                    uuid::Uuid::new_v4().to_string(),
                    text,
                ));
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
    if let Some(crate::state::Overlay::Rewind(ref mut r)) = state.ui.overlay {
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
    if !target_message_id.is_empty()
        && let Some(target_msg) = state
            .session
            .messages
            .iter()
            .find(|m| m.id == target_message_id)
    {
        restored_permission_mode = target_msg.permission_mode;
        // TS `textForResubmit` (`utils/messages.ts:2873-2886`) strips
        // IDE-injected context tags so the restored prompt doesn't
        // leak `<ide_opened_file>` / `<ide_selection>` blocks.
        let stripped = crate::update_rewind::strip_ide_context_tags(target_msg.text_content());
        restored_input_text = Some(stripped).filter(|s| !s.is_empty());
        // TS `restoreMessageSync` (`screens/REPL.tsx:3721-3737`) restores
        // pasted images by reading them off the rewound message. Coco's
        // ChatMessage carries `MessageContent::Image { path }` for
        // pasted images — capture the path so we can re-inject below.
        if let crate::state::MessageContent::Image { path } = &target_msg.content {
            restored_image_path = Some(path.clone());
        }
    }

    if !target_message_id.is_empty()
        && let Some(idx) = state
            .session
            .messages
            .iter()
            .position(|m| m.id == target_message_id)
    {
        state.session.messages.truncate(idx);
    }

    if let Some(mode) = restored_permission_mode {
        state.session.permission_mode = mode;
    }

    if let Some(text) = restored_input_text {
        state.ui.input.text = text;
        state.ui.input.cursor = state.ui.input.text.chars().count() as i32;
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
    // (`screens/REPL.tsx:3721-3737`). Coco's ChatMessage stores at most
    // one image per message; if present, re-attach it; otherwise clear
    // any leftover paste-buffer state so it doesn't leak into the new
    // turn.
    state.ui.paste_manager.clear();
    if let Some(path) = restored_image_path {
        state.ui.paste_manager.add_image(path);
    }

    state.ui.scroll_offset = 0;
    state.ui.user_scrolled = false;
    state.ui.dismiss_overlay();

    let msg = if files_changed > 0 {
        t!("toast.rewound_checkpoint", count = files_changed).to_string()
    } else {
        t!("toast.conversation_rewound_checkpoint").to_string()
    };
    state.ui.add_toast(Toast::success(msg));
    true
}

/// Parse the AskUserQuestion tool input dict into rich
/// `QuestionItem`s the overlay can render.
///
/// Tolerant parser — missing/optional fields use defaults so a
/// model that emits a partial schema still produces a usable overlay
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

/// Extract elicitation fields from a JSON Schema object.
fn parse_elicitation_fields(schema: &serde_json::Value) -> Vec<crate::state::ElicitationField> {
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    props
        .iter()
        .map(|(name, prop)| crate::state::ElicitationField {
            name: name.clone(),
            description: prop
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from),
            value: String::new(),
        })
        .collect()
}
