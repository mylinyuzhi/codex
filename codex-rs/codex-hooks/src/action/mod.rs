//! Hook action system
//!
//! Actions are the pluggable components that execute when a hook is triggered.
//! Different action types support different execution models.

pub mod bash;
pub mod native;
pub mod registry;

use crate::context::HookContext;
use crate::decision::HookResult;
use async_trait::async_trait;
use std::fmt::Debug;

/// Hook action trait
///
/// Actions are executed synchronously (blocking) to allow interception and control flow.
/// While the signature is async to support async operations, the semantics are synchronous:
/// the hook system waits for the action to complete before proceeding.
#[async_trait]
pub trait HookAction: Send + Sync + Debug {
    /// Execute the action
    ///
    /// This method should:
    /// 1. Read the hook context (event data, shared state)
    /// 2. Perform its logic (run script, call function, etc.)
    /// 3. Return a decision and effects
    ///
    /// The action can block execution by returning `HookResult::abort()`.
    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookActionError>;

    /// Human-readable description (for logging and debugging)
    fn description(&self) -> String;

    /// Whether this action can be executed in parallel with others
    ///
    /// - `true`: Safe to run concurrently (no side effects, no shared state)
    /// - `false`: Must run sequentially (e.g., modifies shared resources)
    ///
    /// Default: `true` (safe default for read-only operations)
    fn is_parallelizable(&self) -> bool {
        true
    }
}

/// Errors that can occur during action execution
#[derive(Debug, thiserror::Error)]
pub enum HookActionError {
    #[error("Action execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Action timed out after {0}ms")]
    Timeout(u64),

    #[error("Action output parse error: {0}")]
    ParseError(String),

    #[error("Action not found: {0}")]
    NotFound(String),

    #[error("Action configuration error: {0}")]
    ConfigError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::HookDecision;
    use codex_protocol::hooks::{HookEventContext, HookEventData, HookEventName};

    // Test action for unit tests
    #[derive(Debug)]
    struct TestAction {
        should_block: bool,
    }

    #[async_trait]
    impl HookAction for TestAction {
        async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookActionError> {
            if self.should_block {
                Ok(HookResult::abort("Test block"))
            } else {
                Ok(HookResult::continue_with(vec![]))
            }
        }

        fn description(&self) -> String {
            "Test action".to_string()
        }
    }

    #[tokio::test]
    async fn test_action_execution() {
        let event = HookEventContext {
            session_id: "test".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: HookEventData::Other,
        };

        let ctx = HookContext::new(event);

        // Test continue action
        let action = TestAction {
            should_block: false,
        };
        let result = action.execute(&ctx).await.unwrap();
        assert!(matches!(result.decision, HookDecision::Continue));

        // Test block action
        let action = TestAction { should_block: true };
        let result = action.execute(&ctx).await.unwrap();
        match result.decision {
            HookDecision::Abort { reason } => assert_eq!(reason, "Test block"),
            _ => panic!("Expected Abort decision"),
        }
    }
}
