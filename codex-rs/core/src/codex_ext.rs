//! Extension functions for Codex lifecycle management.
//!
//! This module provides extension functions that hook into Codex lifecycle events
//! without modifying core files directly, minimizing upstream merge conflicts.

use codex_protocol::ConversationId;
use codex_protocol::models::ResponseItem;
use std::path::Path;

use crate::config::system_reminder::LspDiagnosticsMinSeverity;
use crate::config::system_reminder::SystemReminderConfig;
use crate::shell_background::get_global_shell_store;
use crate::subagent::cleanup_stores;
use crate::subagent::get_or_create_stores;
use crate::system_reminder::FileTracker;
use crate::system_reminder::PlanState;
use crate::system_reminder::SystemReminderOrchestrator;
use crate::system_reminder::generator::BackgroundTaskType;
use crate::system_reminder_inject::build_generator_context;
use crate::system_reminder_inject::inject_system_reminders;
use crate::tools::handlers::ext::lsp::get_lsp_diagnostics_store;

/// Clean up session-scoped resources when conversation ends.
///
/// Called from `handlers::shutdown()` in `codex.rs` to ensure proper cleanup
/// of subagent stores (AgentRegistry, BackgroundTaskStore, TranscriptStore)
/// and background shells.
///
/// This prevents memory leaks in long-running server deployments where
/// conversations accumulate without cleanup.
pub fn cleanup_session_resources(conversation_id: &ConversationId) {
    cleanup_stores(conversation_id);

    // Clean up background shells for this conversation
    // This kills running shells and removes all shells associated with this session
    let store = get_global_shell_store();
    store.cleanup_by_conversation(conversation_id);

    // Also clean up old shells from other conversations (time-based fallback)
    // This ensures shells are cleaned even if cleanup_by_conversation missed any
    store.cleanup_old(std::time::Duration::from_secs(3600)); // 1 hour
}

/// Inject system reminders into conversation history.
///
/// This is called before the turn is sent to the model to:
/// - Notify about completed/updated background tasks (shells, agents)
/// - Inject plan mode instructions
/// - Notify about changed files
/// - Include critical instructions
///
/// Returns the task IDs that were notified.
pub async fn run_system_reminder_injection(
    history: &mut Vec<ResponseItem>,
    agent_id: &str,
    is_main_agent: bool,
    cwd: &Path,
    is_plan_mode: bool,
    plan_file_path: Option<&str>,
    conversation_id: Option<&ConversationId>,
    critical_instruction: Option<&str>,
) -> Vec<String> {
    let shell_store = get_global_shell_store();

    // Get or create stores (also provides cached orchestrator)
    // Use get_or_create to ensure orchestrator is available even for new conversations
    let agent_stores = conversation_id.map(|id| get_or_create_stores(*id));

    // Increment inject call count for main agent only
    // This is used by PlanReminderGenerator to determine if reminder should fire
    let current_count = if is_main_agent {
        agent_stores
            .as_ref()
            .map(|s| s.increment_inject_count())
            .unwrap_or(0)
    } else {
        agent_stores
            .as_ref()
            .map(|s| s.get_inject_count())
            .unwrap_or(0)
    };

    // Collect shell tasks (filtered by conversation)
    let mut background_tasks = shell_store.list_for_reminder(conversation_id);

    // Collect subagent tasks (if stores exist for this conversation)
    if let Some(ref stores) = agent_stores {
        background_tasks.extend(stores.background_store.list_for_reminder());
    }

    // NOTE: Do NOT early return here even if background_tasks is empty!
    // Other generators (PlanReminder, ChangedFiles, etc.) need to run regardless.

    // Collect task IDs for marking as notified (grouped by type)
    let notified_ids: Vec<String> = background_tasks
        .iter()
        .filter(|t| !t.notified)
        .map(|t| t.task_id.clone())
        .collect();

    // Use cached orchestrator from stores, or create fallback for edge cases
    let fallback_orchestrator;
    let orchestrator: &SystemReminderOrchestrator = match &agent_stores {
        Some(stores) => &stores.reminder_orchestrator,
        None => {
            fallback_orchestrator =
                SystemReminderOrchestrator::new(SystemReminderConfig::default());
            &fallback_orchestrator
        }
    };

    // Use file tracker from stores for change detection, or fallback to empty
    let fallback_file_tracker;
    let file_tracker: &FileTracker = match &agent_stores {
        Some(stores) => &stores.file_tracker,
        None => {
            fallback_file_tracker = FileTracker::new();
            &fallback_file_tracker
        }
    };

    // Use plan state from stores for reminder tracking, or fallback to empty
    let fallback_plan_state;
    let plan_state: PlanState = match &agent_stores {
        Some(stores) => stores.get_plan_state(),
        None => {
            fallback_plan_state = PlanState::default();
            fallback_plan_state
        }
    };

    // Get LSP diagnostics store if available (lazy initialized on first LSP tool use)
    let diagnostics_store = get_lsp_diagnostics_store();

    let ctx = build_generator_context(
        current_count,
        agent_id,
        is_main_agent,
        true, // has_user_input
        cwd,
        is_plan_mode,
        plan_file_path,
        false, // is_plan_reentry
        file_tracker,
        &plan_state,
        &background_tasks,
        critical_instruction,
        diagnostics_store,
        LspDiagnosticsMinSeverity::default(), // Use default severity filtering (errors only)
    );

    inject_system_reminders(history, orchestrator, &ctx).await;

    // Mark tasks as notified using batch methods for efficiency
    // Group task IDs by type to reduce lock contention
    let shell_ids: Vec<String> = background_tasks
        .iter()
        .filter(|t| !t.notified && t.task_type == BackgroundTaskType::Shell)
        .map(|t| t.task_id.clone())
        .collect();

    let agent_ids: Vec<String> = background_tasks
        .iter()
        .filter(|t| !t.notified && t.task_type == BackgroundTaskType::AsyncAgent)
        .map(|t| t.task_id.clone())
        .collect();

    // Batch mark shells as notified
    if !shell_ids.is_empty() {
        shell_store.mark_all_notified(&shell_ids);
    }

    // Batch mark agents as notified
    if !agent_ids.is_empty() {
        if let Some(ref stores) = agent_stores {
            stores.background_store.mark_all_notified(&agent_ids);
        }
    }

    notified_ids
}

/// Simplified injection for use in codex.rs with minimal parameters.
///
/// This wraps `run_system_reminder_injection` for easier integration.
/// Called on each main agent turn to inject system reminders.
pub async fn maybe_inject_system_reminders(
    history: &mut Vec<ResponseItem>,
    cwd: &Path,
    conversation_id: Option<&ConversationId>,
    critical_instruction: Option<&str>,
) {
    let _ = run_system_reminder_injection(
        history,
        "main",
        true, // is_main_agent
        cwd,
        false, // is_plan_mode
        None,  // plan_file_path
        conversation_id,
        critical_instruction,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::get_or_create_stores;
    use crate::subagent::get_stores;

    #[test]
    fn test_cleanup_session_resources() {
        let conv_id = ConversationId::new();

        // Create stores
        let _ = get_or_create_stores(conv_id);
        assert!(get_stores(&conv_id).is_some());

        // Cleanup
        cleanup_session_resources(&conv_id);

        // Verify cleanup
        assert!(get_stores(&conv_id).is_none());
    }
}
