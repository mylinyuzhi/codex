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
        } => on_diff_stats_loaded(state, message_id, files_changed, insertions, deletions),
        TuiOnlyEvent::RewindCompleted {
            target_message_id,
            files_changed,
        } => on_rewind_completed(state, target_message_id, files_changed),

        // === Question / elicitation / sandbox overlays ===
        TuiOnlyEvent::QuestionAsked {
            request_id,
            message,
        } => {
            state.ui.set_overlay(crate::state::Overlay::Question(
                crate::state::QuestionOverlay {
                    request_id,
                    question: message,
                    options: Vec::new(),
                    selected: 0,
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
    }
}

fn on_diff_stats_loaded(
    state: &mut AppState,
    stats_message_id: String,
    diff_files: i32,
    insertions: i64,
    deletions: i64,
) -> bool {
    let has_any_changes = diff_files > 0;
    if let Some(crate::state::Overlay::Rewind(ref mut r)) = state.ui.overlay {
        let selected_id = r
            .messages
            .get(r.selected as usize)
            .map(|m| m.message_id.as_str());
        if selected_id == Some(&stats_message_id) {
            r.has_file_changes = has_any_changes;
            r.diff_stats = Some(crate::state::DiffStatsPreview {
                files_changed: diff_files,
                insertions,
                deletions,
            });
            r.available_options = crate::state::rewind::build_restore_options(
                r.file_history_enabled,
                has_any_changes,
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

    if !target_message_id.is_empty()
        && let Some(target_msg) = state
            .session
            .messages
            .iter()
            .find(|m| m.id == target_message_id)
    {
        restored_permission_mode = target_msg.permission_mode;
        restored_input_text = Some(target_msg.text_content().to_string()).filter(|s| !s.is_empty());
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
