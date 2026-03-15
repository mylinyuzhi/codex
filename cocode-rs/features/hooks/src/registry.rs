//! Hook registry for storing and dispatching hooks.
//!
//! The `HookRegistry` is the central coordinator: it stores all registered
//! hooks and, when an event occurs, finds the matching hooks and executes them.

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
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
use crate::handlers::inline::InlineHandler;
use crate::result::HookOutcome;
use crate::result::HookResult;
use crate::settings::HookSettings;

/// Callback for LLM model calls (used by Prompt handler in LLM verification mode).
///
/// Args: (system_prompt, user_message) -> Ok(response_text) or Err(error_message)
pub type HookModelCallFn = Arc<
    dyn Fn(String, String) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// Callback for spawning a verification agent (used by Agent handler).
///
/// Args: (prompt, allowed_tools, max_turns) -> Ok(agent_output_text) or Err(error_message)
pub type HookAgentFn = Arc<
    dyn Fn(String, Vec<String>, i32) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

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
    /// Inline handlers stored separately (closures are not serializable).
    /// Keyed by hook name to match against `HookHandler::Inline` definitions.
    inline_handlers: RwLock<HashMap<String, InlineHandler>>,
    /// Settings that control hook execution (e.g., disable_all_hooks).
    settings: RwLock<HookSettings>,
    /// Optional LLM callback for Prompt handler verification mode.
    model_call_fn: RwLock<Option<HookModelCallFn>>,
    /// Optional agent spawn callback for Agent handler verification mode.
    agent_fn: RwLock<Option<HookAgentFn>>,
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
            inline_handlers: RwLock::new(HashMap::new()),
            settings: RwLock::new(HookSettings::default()),
            model_call_fn: RwLock::new(None),
            agent_fn: RwLock::new(None),
        }
    }

    /// Sets the LLM callback for Prompt handler verification mode.
    pub fn set_model_call_fn(&self, f: HookModelCallFn) {
        if let Ok(mut slot) = self.model_call_fn.write() {
            *slot = Some(f);
        }
    }

    /// Sets the agent spawn callback for Agent handler verification mode.
    pub fn set_agent_fn(&self, f: HookAgentFn) {
        if let Ok(mut slot) = self.agent_fn.write() {
            *slot = Some(f);
        }
    }

    /// Returns a clone of the model call callback if set.
    fn get_model_call_fn(&self) -> Option<HookModelCallFn> {
        self.model_call_fn.read().ok().and_then(|slot| slot.clone())
    }

    /// Returns a clone of the agent callback if set.
    fn get_agent_fn(&self) -> Option<HookAgentFn> {
        self.agent_fn.read().ok().and_then(|slot| slot.clone())
    }

    /// Updates the hook settings (e.g., `disable_all_hooks`, `allow_managed_hooks_only`).
    pub fn set_settings(&self, new_settings: HookSettings) {
        if let Ok(mut settings) = self.settings.write() {
            *settings = new_settings;
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

    /// Registers an inline (closure) handler with its hook definition.
    ///
    /// The handler is stored separately from the definition since closures
    /// are not serializable. The hook definition should use `HookHandler::Inline`.
    pub fn register_inline(&self, hook: HookDefinition, handler: InlineHandler) {
        let name = hook.name.clone();
        self.register(hook);
        if let Ok(mut handlers) = self.inline_handlers.write() {
            handlers.insert(name, handler);
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
        // Check settings at execution time (Issue C fix: previously only checked during aggregation)
        if let Ok(settings) = self.settings.read() {
            if settings.disable_all_hooks {
                return Vec::new();
            }
        }

        // Get matching hooks (clone to release lock during execution)
        // The match target varies by event type (tool_name, source, notification_type, etc.)
        let match_target = ctx.match_target();
        let allow_managed_only = self
            .settings
            .read()
            .map(|s| s.allow_managed_hooks_only)
            .unwrap_or(false);

        let matching: Vec<HookDefinition> = if let Ok(hooks) = self.hooks.read() {
            hooks
                .iter()
                .filter(|h| h.enabled && h.event_type == ctx.event_type)
                .filter(|h| {
                    // When allow_managed_hooks_only is set, skip non-managed hooks
                    if allow_managed_only && !h.source.is_managed() {
                        return false;
                    }
                    true
                })
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

        // Pre-execute inline handlers synchronously (closures can't be sent across threads)
        let mut inline_results: HashMap<String, HookResult> = HashMap::new();
        if let Ok(handlers) = self.inline_handlers.read() {
            for hook in &matching {
                if matches!(hook.handler, HookHandler::Inline) {
                    if let Some(handler) = handlers.get(&hook.name) {
                        let result = handler(ctx);
                        inline_results.insert(hook.name.clone(), result);
                    }
                }
            }
        }

        // Snapshot callbacks for use in spawned tasks
        let model_call_fn = self.get_model_call_fn();
        let agent_fn = self.get_agent_fn();

        // Execute all matching hooks in parallel
        let futures: Vec<_> = matching
            .iter()
            .map(|hook| {
                let handler = hook.handler.clone();
                let hook_name = hook.name.clone();
                let timeout_secs = hook.effective_timeout_secs();
                let once = hook.once;
                let ctx = ctx.clone();
                let model_call_fn = model_call_fn.clone();
                let agent_fn = agent_fn.clone();
                // Use pre-computed inline result if available
                let inline_result = inline_results.remove(&hook_name);
                async move {
                    let start = Instant::now();
                    let timeout = tokio::time::Duration::from_secs(timeout_secs as u64);
                    let result = if let Some(r) = inline_result {
                        // Inline handler already executed synchronously
                        Ok(r)
                    } else {
                        tokio::time::timeout(
                            timeout,
                            execute_handler(
                                &handler,
                                &ctx,
                                model_call_fn.as_ref(),
                                agent_fn.as_ref(),
                            ),
                        )
                        .await
                    };

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
            self.remove_once_hooks_by_name(&once_hooks_to_remove);
        }

        outcomes
    }

    /// Removes one-shot hooks by their names.
    ///
    /// Only removes hooks that are both in the `names` set AND have `once: true`,
    /// preventing accidental removal of non-one-shot hooks with the same name.
    fn remove_once_hooks_by_name(&self, names: &[String]) {
        let names_set: HashSet<_> = names.iter().collect();
        if let Ok(mut hooks) = self.hooks.write() {
            let before = hooks.len();
            hooks.retain(|h| !(h.once && names_set.contains(&h.name)));
            let removed = before - hooks.len();
            if removed > 0 {
                info!(
                    count = removed,
                    "Removed one-shot hooks after successful execution"
                );
            }
        }
        // Also clean up inline handlers for removed one-shot hooks
        if let Ok(mut handlers) = self.inline_handlers.write() {
            for name in names {
                handlers.remove(name);
            }
        }
    }

    /// Removes all hooks from a specific source (e.g., when a skill ends).
    pub fn remove_hooks_by_source_name(&self, source_name: &str) {
        let mut removed_names = Vec::new();
        if let Ok(mut hooks) = self.hooks.write() {
            let before = hooks.len();
            hooks.retain(|h| {
                if h.source.name() == Some(source_name) {
                    if matches!(h.handler, HookHandler::Inline) {
                        removed_names.push(h.name.clone());
                    }
                    false
                } else {
                    true
                }
            });
            let removed = before - hooks.len();
            if removed > 0 {
                info!(
                    source = source_name,
                    count = removed,
                    "Removed hooks by source"
                );
            }
        }
        // Clean up inline handlers for removed hooks
        if !removed_names.is_empty() {
            if let Ok(mut handlers) = self.inline_handlers.write() {
                for name in &removed_names {
                    handlers.remove(name);
                }
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
        if let Ok(mut handlers) = self.inline_handlers.write() {
            handlers.clear();
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

    /// Registers a group of hooks with a shared group ID.
    ///
    /// Used by subagent hooks: all hooks in the group share the same
    /// `group_id` so they can be unregistered together when the agent completes.
    pub fn register_group(&self, group_id: &str, hooks: impl IntoIterator<Item = HookDefinition>) {
        if let Ok(mut all) = self.hooks.write() {
            for mut hook in hooks {
                hook.group_id = Some(group_id.to_string());
                info!(
                    name = %hook.name,
                    event = %hook.event_type,
                    group_id,
                    "Registered grouped hook"
                );
                all.push(hook);
            }
        }
    }

    /// Removes all hooks with the given group ID.
    ///
    /// Used to clean up subagent hooks when the agent completes.
    pub fn unregister_group(&self, group_id: &str) {
        if let Ok(mut hooks) = self.hooks.write() {
            let before = hooks.len();
            hooks.retain(|h| h.group_id.as_deref() != Some(group_id));
            let removed = before - hooks.len();
            if removed > 0 {
                info!(group_id, count = removed, "Removed hooks by group");
            }
        }
    }
}

/// Dispatches execution to the appropriate handler.
async fn execute_handler(
    handler: &HookHandler,
    ctx: &HookContext,
    model_call_fn: Option<&HookModelCallFn>,
    agent_fn: Option<&HookAgentFn>,
) -> HookResult {
    match handler {
        HookHandler::Command { command } => {
            // Pass full HookContext to command handler for env vars and stdin JSON
            handlers::command::CommandHandler::execute(command, ctx).await
        }
        HookHandler::Prompt { template, model } => {
            if model.is_some() {
                // LLM verification mode: query the model via callback
                if let Some(call_fn) = model_call_fn {
                    let config = handlers::prompt::PromptVerificationConfig::default();
                    let (system_prompt, user_message) =
                        handlers::prompt::PromptHandler::prepare_verification_request(
                            template, ctx, &config,
                        );
                    match call_fn(system_prompt, user_message).await {
                        Ok(response) => {
                            handlers::prompt::PromptHandler::parse_verification_response(&response)
                        }
                        Err(e) => {
                            warn!("LLM verification call failed: {e}");
                            HookResult::Continue
                        }
                    }
                } else {
                    // No LLM callback available — fall back to template expansion
                    warn!(
                        "Prompt hook has model set but no model_call_fn is registered, falling back to template mode"
                    );
                    let arguments = ctx
                        .tool_input
                        .as_ref()
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    handlers::prompt::PromptHandler::execute(template, &arguments)
                }
            } else {
                // Template-only mode
                let arguments = ctx
                    .tool_input
                    .as_ref()
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                handlers::prompt::PromptHandler::execute(template, &arguments)
            }
        }
        HookHandler::Agent {
            max_turns, prompt, ..
        } => {
            if let Some(spawn_fn) = agent_fn {
                // Real agent execution via callback
                let (config, default_prompt) =
                    handlers::agent::AgentHandler::prepare_verification_request(ctx, *max_turns);
                let effective_prompt = prompt.as_deref().map_or_else(
                    || default_prompt.clone(),
                    |p| {
                        // Expand $ARGUMENTS in custom prompt
                        let ctx_json =
                            serde_json::to_string_pretty(ctx).unwrap_or_else(|_| "{}".to_string());
                        p.replace("$ARGUMENTS", &ctx_json)
                    },
                );
                match spawn_fn(
                    effective_prompt,
                    config.allowed_tools.clone(),
                    config.max_turns,
                )
                .await
                {
                    Ok(response) => {
                        handlers::agent::AgentHandler::parse_verification_response(&response)
                    }
                    Err(e) => {
                        warn!("Agent hook execution failed: {e}");
                        HookResult::Continue
                    }
                }
            } else {
                // No agent callback — stub behavior
                handlers::agent::AgentHandler::execute(*max_turns)
            }
        }
        HookHandler::Webhook { url } => handlers::webhook::WebhookHandler::execute(url, ctx).await,
        HookHandler::Inline => {
            // Inline handlers are pre-executed synchronously in execute().
            // If we reach here, no inline handler was registered for this hook.
            warn!("Inline hook definition has no registered handler closure");
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
