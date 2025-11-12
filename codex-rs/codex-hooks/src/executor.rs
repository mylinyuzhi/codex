//! Hook executor
//!
//! Manages the execution of multiple hook actions, supporting both sequential
//! and parallel execution modes.

use crate::action::{HookAction, HookActionError};
use crate::context::HookContext;
use crate::decision::{HookDecision, HookEffect, HookResult};
use std::sync::Arc;

/// Hook executor
///
/// Coordinates the execution of multiple actions for a single hook point.
pub struct HookExecutor {
    pub(crate) actions: Vec<Arc<dyn HookAction>>,
    sequential: bool,
}

impl HookExecutor {
    /// Create a new executor
    ///
    /// - `actions`: List of actions to execute
    /// - `sequential`: If true, execute sequentially; if false, try parallel execution
    pub fn new(actions: Vec<Arc<dyn HookAction>>, sequential: bool) -> Self {
        Self {
            actions,
            sequential,
        }
    }

    /// Execute all actions
    pub async fn execute(&self, ctx: &HookContext) -> ExecutionResult {
        if self.sequential || !self.all_parallelizable() {
            self.execute_sequential(ctx).await
        } else {
            self.execute_parallel(ctx).await
        }
    }

    /// Check if all actions support parallel execution
    fn all_parallelizable(&self) -> bool {
        self.actions.iter().all(|a| a.is_parallelizable())
    }

    /// Execute actions sequentially
    async fn execute_sequential(&self, ctx: &HookContext) -> ExecutionResult {
        let mut action_results = Vec::new();
        let mut all_effects = Vec::new();

        for action in &self.actions {
            tracing::debug!("Executing hook action: {}", action.description());

            match action.execute(ctx).await {
                Ok(result) => {
                    // Apply effects immediately
                    all_effects.extend(result.effects.clone());
                    apply_effects(ctx, &result.effects).await;

                    // Check if we should stop
                    match &result.decision {
                        HookDecision::Abort { .. }
                        | HookDecision::AskUser { .. }
                        | HookDecision::Retry { .. } => {
                            action_results.push(Ok(result.clone()));
                            return ExecutionResult {
                                action_results,
                                final_decision: result.decision,
                                all_effects,
                            };
                        }
                        HookDecision::Skip => {
                            // Skip this action, continue to next
                            action_results.push(Ok(result));
                            continue;
                        }
                        HookDecision::Continue => {
                            action_results.push(Ok(result));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Hook action failed: {:?}", e);
                    action_results.push(Err(e));
                    // Continue executing other hooks on error (configurable in future)
                }
            }
        }

        ExecutionResult {
            action_results,
            final_decision: HookDecision::Continue,
            all_effects,
        }
    }

    /// Execute actions in parallel
    async fn execute_parallel(&self, ctx: &HookContext) -> ExecutionResult {
        use futures::future::join_all;

        let futures = self.actions.iter().map(|action| {
            let desc = action.description();
            tracing::debug!("Executing hook action (parallel): {}", desc);
            action.execute(ctx)
        });

        let results = join_all(futures).await;

        let mut all_effects = Vec::new();

        // Check if any action blocked execution
        let mut final_decision = HookDecision::Continue;
        for result in &results {
            if let Ok(hook_result) = result {
                all_effects.extend(hook_result.effects.clone());

                match &hook_result.decision {
                    HookDecision::Abort { .. } | HookDecision::AskUser { .. } => {
                        final_decision = hook_result.decision.clone();
                        break;
                    }
                    _ => {}
                }
            }
        }

        // Early return if blocked
        if !matches!(final_decision, HookDecision::Continue) {
            return ExecutionResult {
                action_results: results,
                final_decision,
                all_effects,
            };
        }

        // Apply all effects
        apply_effects(ctx, &all_effects).await;

        ExecutionResult {
            action_results: results,
            final_decision: HookDecision::Continue,
            all_effects,
        }
    }
}

/// Result of executing all actions in an executor
#[derive(Debug)]
pub struct ExecutionResult {
    /// Results from each action
    pub action_results: Vec<Result<HookResult, HookActionError>>,

    /// Final decision (Continue if no action blocked)
    pub final_decision: HookDecision,

    /// All effects collected from actions
    pub all_effects: Vec<HookEffect>,
}

/// Apply effects to the hook context
async fn apply_effects(ctx: &HookContext, effects: &[HookEffect]) {
    let mut state = ctx.state.write().await;

    for effect in effects {
        match effect {
            HookEffect::SetApproved(approved) => {
                state.already_approved = *approved;
            }
            HookEffect::SetSandbox(sandbox) => {
                state.sandbox_type = *sandbox;
            }
            HookEffect::MutateCommand(mutation) => {
                state.command_mutations.push(mutation.clone());
            }
            HookEffect::MutateEnv(env) => {
                state.env_mutations.extend(env.clone());
            }
            HookEffect::AddMetadata { key, value } => {
                state.metadata.insert(key.clone(), value.clone());
            }
            HookEffect::Log { level, message } => {
                // Emit log (doesn't modify state)
                match level {
                    crate::decision::LogLevel::Debug => tracing::debug!("{}", message),
                    crate::decision::LogLevel::Info => tracing::info!("{}", message),
                    crate::decision::LogLevel::Warn => tracing::warn!("{}", message),
                    crate::decision::LogLevel::Error => tracing::error!("{}", message),
                }
            }
            HookEffect::CacheDecision { key, value } => {
                // Store in metadata for now
                state.metadata.insert(
                    format!("cache_{}", key),
                    serde_json::Value::String(value.clone()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::HookAction;
    use async_trait::async_trait;
    use codex_protocol::hooks::{HookEventContext, HookEventData, HookEventName};

    #[derive(Debug)]
    struct TestAction {
        id: String,
        should_block: bool,
    }

    #[async_trait]
    impl HookAction for TestAction {
        async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookActionError> {
            if self.should_block {
                Ok(HookResult::abort(format!("Blocked by {}", self.id)))
            } else {
                Ok(HookResult::continue_with(vec![HookEffect::Log {
                    level: crate::decision::LogLevel::Info,
                    message: format!("Action {} executed", self.id),
                }]))
            }
        }

        fn description(&self) -> String {
            format!("test_{}", self.id)
        }
    }

    fn make_test_context() -> HookContext {
        let event = HookEventContext {
            session_id: "test".to_string(),
            transcript_path: None,
            cwd: "/tmp".to_string(),
            hook_event_name: HookEventName::PreToolUse,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_data: HookEventData::Other,
        };
        HookContext::new(event)
    }

    #[tokio::test]
    async fn test_sequential_execution() {
        let actions: Vec<Arc<dyn HookAction>> = vec![
            Arc::new(TestAction {
                id: "1".to_string(),
                should_block: false,
            }),
            Arc::new(TestAction {
                id: "2".to_string(),
                should_block: false,
            }),
        ];

        let executor = HookExecutor::new(actions, true);
        let ctx = make_test_context();
        let result = executor.execute(&ctx).await;

        assert!(matches!(result.final_decision, HookDecision::Continue));
        assert_eq!(result.action_results.len(), 2);
    }

    #[tokio::test]
    async fn test_execution_stops_on_block() {
        let actions: Vec<Arc<dyn HookAction>> = vec![
            Arc::new(TestAction {
                id: "1".to_string(),
                should_block: false,
            }),
            Arc::new(TestAction {
                id: "2".to_string(),
                should_block: true,
            }),
            Arc::new(TestAction {
                id: "3".to_string(),
                should_block: false,
            }),
        ];

        let executor = HookExecutor::new(actions, true);
        let ctx = make_test_context();
        let result = executor.execute(&ctx).await;

        assert!(matches!(result.final_decision, HookDecision::Abort { .. }));
        // Should stop at action 2
        assert_eq!(result.action_results.len(), 2);
    }

    #[tokio::test]
    async fn test_parallel_execution() {
        let actions: Vec<Arc<dyn HookAction>> = vec![
            Arc::new(TestAction {
                id: "1".to_string(),
                should_block: false,
            }),
            Arc::new(TestAction {
                id: "2".to_string(),
                should_block: false,
            }),
        ];

        let executor = HookExecutor::new(actions, false);
        let ctx = make_test_context();
        let result = executor.execute(&ctx).await;

        assert!(matches!(result.final_decision, HookDecision::Continue));
        assert_eq!(result.action_results.len(), 2);
    }
}
