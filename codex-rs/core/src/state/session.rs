//! Session-wide mutable state.

use codex_protocol::models::ResponseItem;

use crate::codex::SessionConfiguration;
use crate::context_manager::ContextManager;
use crate::protocol::RateLimitSnapshot;
use crate::protocol::TokenUsage;
use crate::protocol::TokenUsageInfo;

/// Persistent, session-scoped state previously stored directly on `Session`.
pub(crate) struct SessionState {
    pub(crate) session_configuration: SessionConfiguration,
    pub(crate) history: ContextManager,
    pub(crate) latest_rate_limits: Option<RateLimitSnapshot>,
    /// Last response_id from server (for previous_response_id field in next request).
    /// With stateless filtering, we only track the ID (not history_len).
    last_response_id: Option<String>,
}

impl SessionState {
    /// Create a new session state mirroring previous `State::default()` semantics.
    pub(crate) fn new(session_configuration: SessionConfiguration) -> Self {
        Self {
            session_configuration,
            history: ContextManager::new(),
            latest_rate_limits: None,
            last_response_id: None,
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

    /// Atomically replace history.
    /// Used by compact and undo operations.
    pub(crate) fn replace_history_and_clear_tracking(&mut self, items: Vec<ResponseItem>) {
        let item_count = items.len();
        let had_response_id = self.last_response_id.is_some();

        if let Some(ref old_id) = self.last_response_id {
            tracing::debug!(
                "Replacing history ({} items) and clearing response_id (was: {})",
                item_count,
                old_id
            );
        } else {
            tracing::debug!(
                "Replacing history ({} items), no response_id to clear",
                item_count
            );
        }

        self.history.replace(items);
        self.last_response_id = None;

        tracing::debug!(
            "History replacement complete: {} items, tracking_cleared={}",
            item_count,
            had_response_id
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

    // Previous response ID tracking (minimal, no history_len)

    /// Set the last response_id from the server.
    /// With stateless filtering, we only track the ID for the next request.
    pub(crate) fn set_last_response(&mut self, response_id: String) {
        tracing::debug!(
            "Setting last_response_id for incremental mode: response_id={}",
            response_id
        );
        self.last_response_id = Some(response_id);
    }

    /// Get the last response_id.
    /// Returns None if no response has completed, or Some(response_id) if available.
    pub(crate) fn get_last_response(&self) -> Option<&str> {
        self.last_response_id.as_deref()
    }

    /// Clear the last response_id.
    /// Used when the tracked response is no longer valid (error recovery, invalidation).
    pub(crate) fn clear_last_response(&mut self) {
        if let Some(ref old_id) = self.last_response_id {
            tracing::debug!("Clearing last_response_id (was: {})", old_id);
        } else {
            tracing::debug!("Clearing last_response_id (was already None)");
        }
        self.last_response_id = None;
    }
}
