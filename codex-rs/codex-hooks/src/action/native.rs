//! Native Rust function action

use super::HookAction;
use super::HookActionError;
use crate::context::HookContext;
use crate::decision::HookResult;
use async_trait::async_trait;
use std::fmt;
use std::sync::Arc;

/// Native hook function type
///
/// A synchronous function that takes a HookContext and returns a HookResult.
/// Wrapped in Arc for cheap cloning.
pub type NativeHookFn = Arc<dyn Fn(&HookContext) -> HookResult + Send + Sync>;

/// Native action that calls a Rust function
#[derive(Clone)]
pub struct NativeAction {
    function_id: String,
    function: NativeHookFn,
}

impl NativeAction {
    pub fn new(function_id: String, function: NativeHookFn) -> Self {
        Self {
            function_id,
            function,
        }
    }

    pub fn function_id(&self) -> &str {
        &self.function_id
    }
}

impl fmt::Debug for NativeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeAction")
            .field("function_id", &self.function_id)
            .finish()
    }
}

#[async_trait]
impl HookAction for NativeAction {
    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookActionError> {
        // Call the function synchronously
        // (We're in async context but the function itself is sync)
        Ok((self.function)(ctx))
    }

    fn description(&self) -> String {
        format!("native: {}", self.function_id)
    }

    fn is_parallelizable(&self) -> bool {
        // Native functions might have side effects, default to sequential
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::HookDecision;
    use crate::decision::HookEffect;
    use codex_protocol::hooks::HookEventContext;
    use codex_protocol::hooks::HookEventData;
    use codex_protocol::hooks::HookEventName;

    #[tokio::test]
    async fn test_native_action() {
        let action = NativeAction::new(
            "test_fn".to_string(),
            Arc::new(|_ctx| HookResult::continue_with(vec![HookEffect::SetApproved(true)])),
        );

        let event = HookEventContext {
            session_id: "test".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: HookEventData::Other,
        };

        let ctx = HookContext::new(event);
        let result = action.execute(&ctx).await.unwrap();

        assert!(matches!(result.decision, HookDecision::Continue));
        assert_eq!(result.effects.len(), 1);
    }
}
