//! Extension functions for Codex lifecycle management.
//!
//! This module provides extension functions that hook into Codex lifecycle events
//! without modifying core files directly, minimizing upstream merge conflicts.

use codex_protocol::ConversationId;

use crate::subagent::cleanup_stores;

/// Clean up session-scoped resources when conversation ends.
///
/// Called from `handlers::shutdown()` in `codex.rs` to ensure proper cleanup
/// of subagent stores (AgentRegistry, BackgroundTaskStore, TranscriptStore).
///
/// This prevents memory leaks in long-running server deployments where
/// conversations accumulate without cleanup.
pub fn cleanup_session_resources(conversation_id: &ConversationId) {
    cleanup_stores(conversation_id);
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
