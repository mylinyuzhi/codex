//! Hook registry for storing and dispatching hooks.
//!
//! The `HookRegistry` is the central coordinator: it stores all registered
//! hooks and, when an event occurs, finds the matching hooks and executes them.

use std::collections::HashSet;
use std::sync::RwLock;
use std::time::Instant;

use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::context::HookContext;
use crate::definition::HookDefinition;
use crate::definition::HookHandler;
use crate::event::HookEventType;
use crate::handlers;
use crate::result::HookOutcome;
use crate::result::HookResult;

/// Central registry that stores hooks and dispatches events.
///
/// The registry supports one-shot hooks (`once: true`) which are automatically
/// removed after successful execution.
///
/// This registry uses interior mutability (`RwLock`) to allow execution through
/// shared references (`Arc<HookRegistry>`), which is needed for concurrent access
/// from the executor.
pub struct HookRegistry {
    hooks: RwLock<Vec<HookDefinition>>,
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HookRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            hooks: RwLock::new(Vec::new()),
        }
    }

    /// Registers a hook definition.
    pub fn register(&self, hook: HookDefinition) {
        info!(
            name = %hook.name,
            event = %hook.event_type,
            once = hook.once,
            "Registered hook"
        );
        if let Ok(mut hooks) = self.hooks.write() {
            hooks.push(hook);
        }
    }

    /// Registers multiple hook definitions.
    pub fn register_all(&self, hooks: impl IntoIterator<Item = HookDefinition>) {
        for hook in hooks {
            self.register(hook);
        }
    }

    /// Returns all hooks registered for a given event type.
    pub fn hooks_for_event(&self, event_type: &HookEventType) -> Vec<HookDefinition> {
        if let Ok(hooks) = self.hooks.read() {
            hooks
                .iter()
                .filter(|h| h.enabled && h.event_type == *event_type)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Executes all matching hooks for the given context.
    ///
    /// Returns outcomes in registration order. A hook matches if:
    /// 1. Its event type equals the context event type.
    /// 2. It is enabled.
    /// 3. Its matcher (if any) matches the context tool name (or no matcher is set).
    ///
    /// One-shot hooks (`once: true`) are removed after successful execution.
    /// They are NOT removed on timeout or failure, allowing retry.
    pub async fn execute(&self, ctx: &HookContext) -> Vec<HookOutcome> {
        // Get matching hooks (clone to release lock during execution)
        // The match target varies by event type (tool_name, source, notification_type, etc.)
        let match_target = ctx.match_target();
        let matching: Vec<HookDefinition> = if let Ok(hooks) = self.hooks.read() {
            hooks
                .iter()
                .filter(|h| h.enabled && h.event_type == ctx.event_type)
                .filter(|h| {
                    match (&h.matcher, match_target) {
                        (Some(matcher), Some(target)) => matcher.matches(target),
                        (Some(_), None) => false, // matcher present but no target to match against
                        (None, _) => true,        // no matcher means always match
                    }
                })
                .cloned()
                .collect()
        } else {
            return Vec::new();
        };

        // Execute all matching hooks in parallel
        let futures: Vec<_> = matching
            .iter()
            .map(|hook| {
                let handler = hook.handler.clone();
                let hook_name = hook.name.clone();
                let timeout_secs = hook.effective_timeout_secs();
                let once = hook.once;
                let ctx = ctx.clone();
                async move {
                    let start = Instant::now();
                    let timeout = tokio::time::Duration::from_secs(timeout_secs as u64);
                    let result =
                        tokio::time::timeout(timeout, execute_handler(&handler, &ctx)).await;

                    let duration_ms = start.elapsed().as_millis() as i64;

                    let (result, is_success) = match result {
                        Ok(r) => {
                            let success = !matches!(r, HookResult::Reject { .. });
                            (r, success)
                        }
                        Err(_) => {
                            warn!(
                                hook_name = %hook_name,
                                timeout_secs,
                                "Hook timed out"
                            );
                            (HookResult::Continue, false)
                        }
                    };

                    info!(
                        hook_name = %hook_name,
                        duration_ms,
                        once,
                        success = is_success,
                        "Hook executed"
                    );

                    (
                        HookOutcome {
                            hook_name: hook_name.clone(),
                            result,
                            duration_ms,
                        },
                        once && is_success,
                        hook_name,
                    )
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        let mut outcomes = Vec::with_capacity(results.len());
        let mut once_hooks_to_remove: Vec<String> = Vec::new();

        for (outcome, should_remove, name) in results {
            if should_remove {
                debug!(hook_name = %name, "One-shot hook will be removed");
                once_hooks_to_remove.push(name);
            }
            outcomes.push(outcome);
        }

        // Remove one-shot hooks that executed successfully
        if !once_hooks_to_remove.is_empty() {
            self.remove_hooks_by_name(&once_hooks_to_remove);
        }

        outcomes
    }

    /// Removes hooks by their names.
    fn remove_hooks_by_name(&self, names: &[String]) {
        let names_set: HashSet<_> = names.iter().collect();
        if let Ok(mut hooks) = self.hooks.write() {
            let before = hooks.len();
            hooks.retain(|h| !names_set.contains(&h.name));
            let removed = before - hooks.len();
            if removed > 0 {
                info!(
                    count = removed,
                    "Removed one-shot hooks after successful execution"
                );
            }
        }
    }

    /// Removes all hooks from a specific source (e.g., when a skill ends).
    pub fn remove_hooks_by_source_name(&self, source_name: &str) {
        if let Ok(mut hooks) = self.hooks.write() {
            let before = hooks.len();
            hooks.retain(|h| h.source.name() != Some(source_name));
            let removed = before - hooks.len();
            if removed > 0 {
                info!(
                    source = source_name,
                    count = removed,
                    "Removed hooks by source"
                );
            }
        }
    }

    /// Removes all hooks with the specified scope.
    pub fn remove_hooks_by_scope(&self, scope: crate::scope::HookScope) {
        if let Ok(mut hooks) = self.hooks.write() {
            let before = hooks.len();
            hooks.retain(|h| h.source.scope() != scope);
            let removed = before - hooks.len();
            if removed > 0 {
                info!(scope = %scope, count = removed, "Removed hooks by scope");
            }
        }
    }

    /// Removes all registered hooks.
    pub fn clear(&self) {
        if let Ok(mut hooks) = self.hooks.write() {
            hooks.clear();
        }
    }

    /// Returns the number of registered hooks.
    pub fn len(&self) -> usize {
        self.hooks.read().map(|h| h.len()).unwrap_or(0)
    }

    /// Returns `true` if no hooks are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a copy of all registered hooks.
    pub fn all_hooks(&self) -> Vec<HookDefinition> {
        self.hooks.read().map(|h| h.clone()).unwrap_or_default()
    }
}

/// Dispatches execution to the appropriate handler.
async fn execute_handler(handler: &HookHandler, ctx: &HookContext) -> HookResult {
    match handler {
        HookHandler::Command { command, args } => {
            // Pass full HookContext to command handler for env vars and stdin JSON
            handlers::command::CommandHandler::execute(command, args, ctx).await
        }
        // NOTE: `model` field is ignored — LLM verification mode is not yet implemented.
        // When `model` is set, this should call an LLM via a callback instead of
        // template expansion. See `PromptHandler::prepare_verification_request`.
        HookHandler::Prompt { template, .. } => {
            let arguments = ctx
                .tool_input
                .as_ref()
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            handlers::prompt::PromptHandler::execute(template, &arguments)
        }
        // NOTE: `prompt` and `timeout` fields are ignored — agent handler is a stub.
        // Full implementation requires a `SpawnAgentFn` callback injected into
        // `HookRegistry`. See `AgentHandler::prepare_verification_request`.
        HookHandler::Agent { max_turns, .. } => handlers::agent::AgentHandler::execute(*max_turns),
        HookHandler::Webhook { url } => handlers::webhook::WebhookHandler::execute(url, ctx).await,
        HookHandler::Inline => {
            warn!("Inline handler cannot be dispatched through the registry");
            HookResult::Continue
        }
    }
}

impl std::fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookRegistry")
            .field("hooks_count", &self.len())
            .finish()
    }
}

#[cfg(test)]
#[path = "registry.test.rs"]
mod tests;
