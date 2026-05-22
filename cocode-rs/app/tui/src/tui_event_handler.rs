//! Handler for TUI-only events from `CoreEvent::Tui`.
//!
//! Processes [`TuiEvent`] variants that drive overlay displays, progress
//! indicators, and other interactive UI elements with no protocol equivalent.

use cocode_protocol::tui_event::TuiEvent;

use crate::i18n::t;
use crate::state::AppState;
use crate::state::Overlay;
use crate::state::PermissionOverlay;
use crate::state::SandboxPermissionOverlay;

/// Handle a TUI-only event and update the application state.
pub fn handle_tui_event(state: &mut AppState, event: TuiEvent) {
    match event {
        // ══════════════════════════════════════════════════════════════
        // Approval/question/elicitation overlays
        // ══════════════════════════════════════════════════════════════
        TuiEvent::ApprovalRequired { request } => {
            if request.tool_name == cocode_protocol::ToolName::ExitPlanMode.as_str() {
                state.ui.set_overlay(Overlay::PlanExitApproval(
                    crate::state::PlanExitOverlay::new(request, state.session.bypass_available),
                ));
            } else {
                state
                    .ui
                    .set_overlay(Overlay::Permission(PermissionOverlay::new(request)));
            }
        }
        TuiEvent::QuestionAsked {
            request_id,
            questions,
        } => {
            state
                .ui
                .set_overlay(Overlay::Question(crate::state::QuestionOverlay::new(
                    request_id, &questions,
                )));
        }
        TuiEvent::ElicitationRequested {
            request_id,
            server_name,
            message,
            mode,
            schema,
            url,
        } => {
            state.ui.set_overlay(Overlay::Elicitation(
                crate::state::ElicitationOverlay::from_request(
                    request_id,
                    server_name,
                    message,
                    &mode,
                    schema.as_ref(),
                    url,
                ),
            ));
        }
        TuiEvent::SandboxApprovalRequired {
            request,
            access_type,
        } => {
            state
                .ui
                .set_overlay(Overlay::SandboxPermission(SandboxPermissionOverlay::new(
                    request,
                    access_type,
                )));
        }

        // ══════════════════════════════════════════════════════════════
        // Tool call delta and progress (TUI-only tracking)
        // ══════════════════════════════════════════════════════════════
        TuiEvent::ToolCallDelta { call_id, delta } => {
            state.ui.append_tool_call_delta(&call_id, &delta);
        }
        TuiEvent::ToolProgress { call_id, progress } => {
            if let Some(msg) = progress.message {
                state.session.update_tool_progress(&call_id, msg);
            }
        }
        TuiEvent::ToolExecutionAborted { reason } => {
            state
                .ui
                .toast_warning(format!("{}: {reason}", t!("toast.tool_aborted")));
        }

        // ══════════════════════════════════════════════════════════════
        // Overlay data events
        // ══════════════════════════════════════════════════════════════
        TuiEvent::PluginDataReady {
            installed,
            marketplaces,
        } => {
            use crate::state::MarketplaceSummary;
            use crate::state::PluginSummary;
            let installed_items: Vec<PluginSummary> = installed
                .into_iter()
                .map(|p| PluginSummary {
                    name: p.name,
                    description: p.description,
                    version: p.version,
                    enabled: p.enabled,
                    scope: p.scope,
                    skills_count: p.skills_count,
                    hooks_count: p.hooks_count,
                    agents_count: p.agents_count,
                })
                .collect();
            let marketplace_items: Vec<MarketplaceSummary> = marketplaces
                .into_iter()
                .map(|m| MarketplaceSummary {
                    name: m.name,
                    source_type: m.source_type,
                    source: m.source,
                    auto_update: m.auto_update,
                    plugin_count: m.plugin_count,
                })
                .collect();
            state.ui.set_overlay(Overlay::PluginManager(
                crate::state::PluginManagerOverlay::new(
                    installed_items,
                    marketplace_items,
                    Vec::new(),
                ),
            ));
        }
        TuiEvent::OutputStylesReady { styles } => {
            use crate::state::OutputStylePickerItem;
            let items: Vec<OutputStylePickerItem> = styles
                .into_iter()
                .map(|s| OutputStylePickerItem {
                    name: s.name,
                    source: s.source,
                    description: s.description,
                })
                .collect();
            if items.is_empty() {
                state
                    .ui
                    .toast_info(t!("toast.no_output_styles").to_string());
            } else {
                state.ui.set_overlay(Overlay::OutputStylePicker(
                    crate::state::OutputStylePickerOverlay::new(items),
                ));
            }
        }
        TuiEvent::RewindCheckpointsReady { checkpoints } => {
            if checkpoints.is_empty() {
                state
                    .ui
                    .toast_info(t!("toast.rewind_no_checkpoints").to_string());
            } else {
                let mut overlay = crate::state::RewindSelectorOverlay::new(checkpoints);
                overlay.needs_initial_diff_stats = true;
                state.ui.set_overlay(Overlay::RewindSelector(overlay));
            }
        }
        TuiEvent::DiffStatsReady { turn_number, stats } => {
            if let Some(Overlay::RewindSelector(ref mut rw)) = state.ui.overlay {
                for cp in &mut rw.checkpoints {
                    if cp.turn_number == turn_number {
                        cp.diff_stats = Some(stats);
                        break;
                    }
                }
            }
        }

        // ══════════════════════════════════════════════════════════════
        // Toast/state events
        // ══════════════════════════════════════════════════════════════
        TuiEvent::CompactionCircuitBreakerOpen {
            consecutive_failures,
        } => {
            state.ui.toast_warning(format!(
                "{} ({consecutive_failures})",
                t!("toast.compaction_circuit_breaker")
            ));
        }
        TuiEvent::MicroCompactionApplied {
            removed_results,
            tokens_saved,
        } => {
            state.ui.toast_info(
                t!(
                    "toast.micro_compaction",
                    count = removed_results,
                    tokens = tokens_saved
                )
                .to_string(),
            );
        }
        TuiEvent::SessionMemoryCompactApplied { saved_tokens, .. } => {
            state
                .ui
                .toast_info(t!("toast.session_memory_compact", saved = saved_tokens).to_string());
        }
        TuiEvent::SessionMemoryExtractionStarted { .. } => {
            state
                .ui
                .toast_info(t!("toast.session_memory_started").to_string());
        }
        TuiEvent::SessionMemoryExtractionCompleted { .. } => {
            state
                .ui
                .toast_success(t!("toast.session_memory_completed").to_string());
        }
        TuiEvent::SessionMemoryExtractionFailed { error, .. } => {
            tracing::error!(error, "Session memory extraction failed");
            state
                .ui
                .toast_error(t!("toast.session_memory_failed").to_string());
        }
        TuiEvent::SpeculativeRolledBack { reason, .. } => {
            state
                .ui
                .toast_warning(t!("toast.speculative_rolled_back", reason = reason).to_string());
        }
        TuiEvent::CronJobDisabled {
            job_id,
            consecutive_failures,
        } => {
            state.ui.toast_warning(
                t!(
                    "toast.cron_job_disabled",
                    job_id = job_id,
                    failures = consecutive_failures
                )
                .to_string(),
            );
        }
        TuiEvent::CronJobsMissed { count, summary } => {
            state.ui.toast_info(
                t!("toast.cron_jobs_missed", count = count, summary = summary).to_string(),
            );
        }
    }
}

#[cfg(test)]
#[path = "tui_event_handler.test.rs"]
mod tests;
