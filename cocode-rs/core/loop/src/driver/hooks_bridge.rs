//! Hook execution bridge methods for the agent loop.

use cocode_protocol::LoopEvent;
use tracing::info;

use super::AgentLoop;

impl AgentLoop {
    /// Execute lifecycle hooks (non-tool events) and emit HookExecuted for each.
    ///
    /// Used for SessionStart, UserPromptSubmit, Stop, SessionEnd, etc.
    /// Returns `true` if any hook rejected (for events that support rejection).
    pub(crate) async fn execute_lifecycle_hooks(&self, ctx: cocode_hooks::HookContext) -> bool {
        let outcomes = self.hooks.execute(&ctx).await;
        let mut rejected = false;

        for outcome in &outcomes {
            self.emit(LoopEvent::HookExecuted {
                hook_type: ctx.event_type.clone(),
                hook_name: outcome.hook_name.clone(),
            })
            .await;

            match &outcome.result {
                cocode_hooks::HookResult::Reject { reason } => {
                    info!(
                        hook_name = %outcome.hook_name,
                        reason = %reason,
                        event = %ctx.event_type,
                        "Lifecycle hook rejected"
                    );
                    rejected = true;
                }
                cocode_hooks::HookResult::Async { task_id, hook_name } => {
                    self.async_hook_tracker
                        .register(task_id.clone(), hook_name.clone());
                }
                cocode_hooks::HookResult::ContinueWithContext {
                    additional_context,
                    env_vars,
                } => {
                    if let Some(ctx_str) = additional_context {
                        info!(
                            hook_name = %outcome.hook_name,
                            event = %ctx.event_type,
                            "Lifecycle hook provided additional context: {ctx_str}"
                        );
                    }
                    // Propagate env vars from SessionStart hooks to the shell executor
                    if !env_vars.is_empty() {
                        info!(
                            hook_name = %outcome.hook_name,
                            count = env_vars.len(),
                            "SessionStart hook provided env vars for shell overlay"
                        );
                        self.shell_executor.add_env_overlay(env_vars.clone());
                    }
                }
                cocode_hooks::HookResult::SystemMessage { message } => {
                    info!(
                        hook_name = %outcome.hook_name,
                        event = %ctx.event_type,
                        "Lifecycle hook system message: {message}"
                    );
                }
                _ => {}
            }
        }

        rejected
    }

    /// Fire a Notification hook (informational, non-blocking).
    pub(crate) async fn fire_notification_hook(
        &self,
        notification_type: &str,
        title: &str,
        message: &str,
    ) {
        let ctx = cocode_hooks::HookContext::new(
            cocode_hooks::HookEventType::Notification,
            uuid::Uuid::new_v4().to_string(),
            self.context.environment.cwd.clone(),
        )
        .with_notification_type(notification_type)
        .with_title(title)
        .with_message(message);

        let outcomes = self.hooks.execute(&ctx).await;
        for outcome in &outcomes {
            self.emit(LoopEvent::HookExecuted {
                hook_type: ctx.event_type.clone(),
                hook_name: outcome.hook_name.clone(),
            })
            .await;
        }
    }
}
