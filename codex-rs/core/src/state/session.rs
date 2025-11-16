//! Session-wide mutable state.

use codex_protocol::models::ResponseItem;

use crate::codex::SessionConfiguration;
use crate::context_manager::ContextManager;
use crate::protocol::RateLimitSnapshot;
use crate::protocol::TokenUsage;
use crate::protocol::TokenUsageInfo;

/// Tracks the state when a response completed, enabling incremental input optimization.
/// Both response_id and history_len must be set atomically to ensure consistency.
#[derive(Debug, Clone)]
struct LastResponseTracker {
    response_id: String,
    history_len: usize,
}

/// Persistent, session-scoped state previously stored directly on `Session`.
pub(crate) struct SessionState {
    pub(crate) session_configuration: SessionConfiguration,
    pub(crate) history: ContextManager,
    pub(crate) latest_rate_limits: Option<RateLimitSnapshot>,
    /// Tracks last response completion state for incremental input optimization.
    /// Contains both response_id and history length at completion time.
    /// Cleared on: compact, model switch, undo, error recovery.
    last_response: Option<LastResponseTracker>,
}

impl SessionState {
    /// Create a new session state mirroring previous `State::default()` semantics.
    pub(crate) fn new(session_configuration: SessionConfiguration) -> Self {
        Self {
            session_configuration,
            history: ContextManager::new(),
            latest_rate_limits: None,
            last_response: None,
        }
    }

    // History helpers
    pub(crate) fn record_items<I>(&mut self, items: I)
    where
        I: IntoIterator,
        I::Item: std::ops::Deref<Target = ResponseItem>,
    {
        self.history.record_items(items)
    }

    pub(crate) fn clone_history(&self) -> ContextManager {
        self.history.clone()
    }

    /// Atomically replace history and clear response tracking.
    /// Used by compact and undo operations to ensure consistency.
    pub(crate) fn replace_history_and_clear_tracking(&mut self, items: Vec<ResponseItem>) {
        let item_count = items.len();
        self.history.replace(items);
        self.clear_last_response();
        tracing::debug!(
            "History replaced ({} items) and tracking cleared atomically",
            item_count
        );
    }

    // Token/rate limit helpers
    pub(crate) fn update_token_info_from_usage(
        &mut self,
        usage: &TokenUsage,
        model_context_window: Option<i64>,
    ) {
        self.history.update_token_info(usage, model_context_window);
    }

    pub(crate) fn token_info(&self) -> Option<TokenUsageInfo> {
        self.history.token_info()
    }

    pub(crate) fn set_rate_limits(&mut self, snapshot: RateLimitSnapshot) {
        self.latest_rate_limits = Some(snapshot);
    }

    pub(crate) fn token_info_and_rate_limits(
        &self,
    ) -> (Option<TokenUsageInfo>, Option<RateLimitSnapshot>) {
        (self.token_info(), self.latest_rate_limits.clone())
    }

    pub(crate) fn set_token_usage_full(&mut self, context_window: i64) {
        self.history.set_token_usage_full(context_window);
    }

    // Previous response tracking helpers

    /// Atomically set both response_id and history_len when a response completes.
    /// This ensures the two values are always consistent.
    pub(crate) fn set_last_response(&mut self, response_id: String, history_len: usize) {
        self.last_response = Some(LastResponseTracker {
            response_id,
            history_len,
        });
    }

    /// Atomically capture current history length and set last_response tracking.
    /// This is the preferred method to avoid race conditions between getting history length
    /// and setting the tracking data.
    pub(crate) fn set_last_response_from_current_history(&mut self, response_id: String) {
        let history_len = self.history.get_history().len();
        tracing::debug!(
            "Tracking set: response_id={}, history_len={} (for next turn's incremental input)",
            response_id,
            history_len
        );
        self.set_last_response(response_id, history_len);
    }

    /// Get both response_id and history_len atomically.
    /// Returns None if no response has completed, or Some((response_id, history_len)) if available.
    pub(crate) fn get_last_response(&self) -> Option<(&str, usize)> {
        self.last_response
            .as_ref()
            .map(|tracker| (tracker.response_id.as_str(), tracker.history_len))
    }

    /// Clear all response tracking data atomically.
    /// Called when: compact, model switch, undo, or error recovery.
    pub(crate) fn clear_last_response(&mut self) {
        self.last_response = None;
    }
}
