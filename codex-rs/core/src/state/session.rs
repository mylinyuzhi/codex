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
    /// Last response ID from the model, used for continuing conversations.
    /// Cleared on compact or model switch.
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

    pub(crate) fn replace_history(&mut self, items: Vec<ResponseItem>) {
        self.history.replace(items);
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

    // Previous response ID helpers
    pub(crate) fn set_last_response_id(&mut self, id: String) {
        self.last_response_id = Some(id);
    }

    pub(crate) fn get_last_response_id(&self) -> Option<&str> {
        self.last_response_id.as_deref()
    }

    pub(crate) fn clear_last_response_id(&mut self) {
        self.last_response_id = None;
    }
}
