//! Session-scoped subagent stores with global registry.
//!
//! This module provides a global registry pattern for managing subagent stores
//! keyed by conversation_id. This avoids modifying Session/codex.rs while
//! ensuring stores persist across turns within a session.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

// Note: inject_call_count uses Ordering::Relaxed because:
// 1. We only need monotonic increment, not synchronization
// 2. Exact count doesn't need to be synchronized across threads

use dashmap::DashMap;

use super::AgentRegistry;
use super::BackgroundTaskStore;
use super::TranscriptStore;
use crate::config::system_reminder::SystemReminderConfig;
use crate::system_reminder::FileTracker;
use crate::system_reminder::PlanState;
use crate::system_reminder::PlanStep;
use crate::system_reminder::SystemReminderOrchestrator;
use codex_protocol::ConversationId;
use codex_protocol::plan_tool::UpdatePlanArgs;

/// Session-scoped subagent stores.
///
/// These stores maintain state that must persist across turns within a session:
/// - AgentRegistry: Caches loaded agent definitions
/// - BackgroundTaskStore: Tracks background subagent tasks
/// - TranscriptStore: Records agent transcripts for resume functionality
/// - ReminderOrchestrator: Cached system reminder orchestrator (avoids per-turn allocation)
/// - FileTracker: Tracks file reads for change detection
/// - PlanState: Tracks plan state for reminder generation
/// - inject_call_count: Tracks main agent reminder injection calls
#[derive(Debug)]
pub struct SubagentStores {
    pub registry: Arc<AgentRegistry>,
    pub background_store: Arc<BackgroundTaskStore>,
    pub transcript_store: Arc<TranscriptStore>,
    pub reminder_orchestrator: Arc<SystemReminderOrchestrator>,
    pub file_tracker: Arc<FileTracker>,
    pub plan_state: Arc<RwLock<PlanState>>,
    /// Counter for main agent reminder injection calls.
    /// Used by PlanReminderGenerator to determine if reminder should fire.
    inject_call_count: AtomicI32,
}

/// Build default search paths for custom agent discovery.
///
/// Search order:
/// 1. `~/.config/codex/agents/` - User config directory
/// 2. `~/.codex/agents/` - User home directory
/// 3. `.codex/agents/` - Project local directory
fn build_default_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. User config directory (~/.config/codex/agents/ on Linux/macOS)
    if let Some(config_dir) = dirs::config_dir() {
        paths.push(config_dir.join("codex").join("agents"));
    }

    // 2. User home directory (~/.codex/agents/)
    if let Some(home_dir) = dirs::home_dir() {
        paths.push(home_dir.join(".codex").join("agents"));
    }

    // 3. Project local directory (.codex/agents/)
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".codex").join("agents"));
    }

    paths
}

impl SubagentStores {
    pub fn new() -> Self {
        let search_paths = build_default_search_paths();
        Self {
            registry: Arc::new(AgentRegistry::with_search_paths(search_paths)),
            background_store: Arc::new(BackgroundTaskStore::new()),
            transcript_store: Arc::new(TranscriptStore::new()),
            reminder_orchestrator: Arc::new(SystemReminderOrchestrator::new(
                SystemReminderConfig::default(),
            )),
            file_tracker: Arc::new(FileTracker::new()),
            plan_state: Arc::new(RwLock::new(PlanState::default())),
            inject_call_count: AtomicI32::new(0),
        }
    }

    /// Increment and return the new inject call count.
    /// Only call this for main agent turns.
    pub fn increment_inject_count(&self) -> i32 {
        self.inject_call_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Get the current inject call count.
    pub fn get_inject_count(&self) -> i32 {
        self.inject_call_count.load(Ordering::Relaxed)
    }

    /// Update plan state from UpdatePlanArgs.
    ///
    /// Called by the update_plan handler to track plan state for reminder generation.
    pub fn update_plan_state(&self, args: &UpdatePlanArgs, current_count: i32) {
        let mut state = self.plan_state.write().expect("plan_state lock poisoned");
        state.steps = args
            .plan
            .iter()
            .map(|item| PlanStep {
                step: item.step.clone(),
                status: format!("{:?}", item.status).to_lowercase(),
            })
            .collect();
        state.is_empty = state.steps.is_empty();
        state.last_update_count = current_count;
    }

    /// Get a snapshot of the current plan state.
    pub fn get_plan_state(&self) -> PlanState {
        self.plan_state
            .read()
            .expect("plan_state lock poisoned")
            .clone()
    }
}

impl Default for SubagentStores {
    fn default() -> Self {
        Self::new()
    }
}

/// Global registry mapping conversation_id to session-scoped stores.
///
/// Using LazyLock + DashMap for thread-safe lazy initialization with
/// concurrent access support.
static STORES_REGISTRY: LazyLock<DashMap<ConversationId, Arc<SubagentStores>>> =
    LazyLock::new(DashMap::new);

/// Get or create stores for a session by conversation_id.
///
/// This is the main entry point for handlers to access session-scoped stores.
/// The stores are created on first access and reused for subsequent calls
/// with the same conversation_id.
///
/// # Example
/// ```ignore
/// let stores = get_or_create_stores(session.conversation_id);
/// // Use stores.background_store, stores.transcript_store, etc.
/// ```
pub fn get_or_create_stores(conversation_id: ConversationId) -> Arc<SubagentStores> {
    STORES_REGISTRY
        .entry(conversation_id)
        .or_insert_with(|| Arc::new(SubagentStores::new()))
        .clone()
}

/// Cleanup stores when session ends.
///
/// Should be called when a session is terminated to free memory.
/// Not calling this won't cause memory leaks for short-lived processes,
/// but long-running servers should call this on session cleanup.
pub fn cleanup_stores(conversation_id: &ConversationId) {
    STORES_REGISTRY.remove(conversation_id);
}

/// Get stores if they exist (without creating new ones).
///
/// Useful for operations that should only work on existing sessions.
pub fn get_stores(conversation_id: &ConversationId) -> Option<Arc<SubagentStores>> {
    STORES_REGISTRY.get(conversation_id).map(|r| r.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_default_search_paths() {
        let paths = build_default_search_paths();

        // Should have at least project local path
        assert!(!paths.is_empty());

        // All paths should end with "agents"
        for path in &paths {
            assert!(
                path.ends_with("agents"),
                "Path should end with 'agents': {path:?}"
            );
        }
    }

    #[test]
    fn test_get_or_create_stores() {
        let conv_id = ConversationId::new();

        // First access creates stores
        let stores1 = get_or_create_stores(conv_id);

        // Second access returns same stores
        let stores2 = get_or_create_stores(conv_id);

        // Both should point to same Arc
        assert!(Arc::ptr_eq(&stores1, &stores2));

        // Cleanup
        cleanup_stores(&conv_id);

        // After cleanup, get_stores returns None
        assert!(get_stores(&conv_id).is_none());
    }

    #[test]
    fn test_different_sessions_have_different_stores() {
        let conv_id1 = ConversationId::new();
        let conv_id2 = ConversationId::new();

        let stores1 = get_or_create_stores(conv_id1);
        let stores2 = get_or_create_stores(conv_id2);

        // Different sessions should have different stores
        assert!(!Arc::ptr_eq(&stores1, &stores2));

        // Cleanup
        cleanup_stores(&conv_id1);
        cleanup_stores(&conv_id2);
    }
}
