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

use crate::async_tracker::AsyncHookTracker;
use crate::context::HookContext;
use crate::definition::HookDefinition;
use crate::definition::HookHandler;
use crate::event::HookEventType;
use crate::handlers;
use crate::handlers::inline::InlineHandler;
use crate::lock_utils::lock_read;
use crate::lock_utils::lock_write;
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

/// Callback for routing hook execution to an SDK client.
///
/// Args: (callback_id, event_type, hook_context_json) -> Ok(output_json) or Err(error)
pub type HookSdkCallbackFn = Arc<
    dyn Fn(
            String,
            String,
            serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send>>
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
    /// Optional SDK callback for routing hooks to the SDK client.
    sdk_callback_fn: RwLock<Option<HookSdkCallbackFn>>,
    /// Optional async hook tracker for completing background hook tasks.
    ///
    /// When set, background hooks will call `tracker.complete()` when they finish,
    /// allowing their results to be delivered via system reminders.
    async_tracker: RwLock<Option<Arc<AsyncHookTracker>>>,
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
            sdk_callback_fn: RwLock::new(None),
            async_tracker: RwLock::new(None),
        }
    }

    /// Sets the async hook tracker for completing background hook tasks.
    ///
    /// When set, hooks that are backgrounded via config-based async will call
    /// `tracker.complete()` when they finish, enabling result delivery via
    /// system reminders.
    pub fn set_async_hook_tracker(&self, tracker: Arc<AsyncHookTracker>) {
        *lock_write(&self.async_tracker, "async_tracker") = Some(tracker);
    }

    /// Sets the LLM callback for Prompt handler verification mode.
    pub fn set_model_call_fn(&self, f: HookModelCallFn) {
        *lock_write(&self.model_call_fn, "model_call_fn") = Some(f);
    }

    /// Sets the agent spawn callback for Agent handler verification mode.
    pub fn set_agent_fn(&self, f: HookAgentFn) {
        *lock_write(&self.agent_fn, "agent_fn") = Some(f);
    }

    /// Sets the SDK callback for routing hooks to the SDK client.
    pub fn set_sdk_callback_fn(&self, f: HookSdkCallbackFn) {
        *lock_write(&self.sdk_callback_fn, "sdk_callback_fn") = Some(f);
    }

    /// Returns a clone of the model call callback if set.
    fn get_model_call_fn(&self) -> Option<HookModelCallFn> {
        lock_read(&self.model_call_fn, "model_call_fn").clone()
    }

    /// Returns a clone of the agent callback if set.
    fn get_agent_fn(&self) -> Option<HookAgentFn> {
        lock_read(&self.agent_fn, "agent_fn").clone()
    }

    /// Returns a clone of the SDK callback if set.
    fn get_sdk_callback_fn(&self) -> Option<HookSdkCallbackFn> {
        lock_read(&self.sdk_callback_fn, "sdk_callback_fn").clone()
    }

    /// Updates the hook settings (e.g., `disable_all_hooks`, `allow_managed_hooks_only`).
    pub fn set_settings(&self, new_settings: HookSettings) {
        *lock_write(&self.settings, "settings") = new_settings;
    }

    /// Registers a hook definition.
    pub fn register(&self, hook: HookDefinition) {
        info!(
            name = %hook.name,
            event = %hook.event_type,
            once = hook.once,
            "Registered hook"
        );
        lock_write(&self.hooks, "hooks").push(hook);
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
        lock_write(&self.inline_handlers, "inline_handlers").insert(name, handler);
    }

    /// Returns all hooks registered for a given event type.
    pub fn hooks_for_event(&self, event_type: &HookEventType) -> Vec<HookDefinition> {
        lock_read(&self.hooks, "hooks")
            .iter()
            .filter(|h| h.enabled && h.event_type == *event_type)
            .cloned()
            .collect()
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
        // Snapshot settings once to avoid holding lock across filtering
        let (disable_all, allow_managed_only, workspace_trusted) = {
            let settings = lock_read(&self.settings, "settings");
            (
                settings.disable_all_hooks,
                settings.allow_managed_hooks_only,
                settings.workspace_trusted,
            )
        };
        if disable_all {
            return Vec::new();
        }

        // Get matching hooks (clone to release lock during execution)
        // The match target varies by event type (tool_name, source, notification_type, etc.)
        let match_target = ctx.match_target();

        let matching: Vec<HookDefinition> = lock_read(&self.hooks, "hooks")
            .iter()
            .filter(|h| h.enabled && h.event_type == ctx.event_type)
            .filter(|h| {
                // When allow_managed_hooks_only is set, or workspace is untrusted,
                // skip non-managed hooks
                if (allow_managed_only || !workspace_trusted) && !h.source.is_managed() {
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
            .collect();

        // Pre-execute inline handlers synchronously (closures can't be sent across threads)
        let mut inline_results: HashMap<String, HookResult> = HashMap::new();
        {
            let handlers = lock_read(&self.inline_handlers, "inline_handlers");
            for hook in &matching {
                if matches!(hook.handler, HookHandler::Inline)
                    && let Some(handler) = handlers.get(&hook.name)
                {
                    let result = handler(ctx);
                    inline_results.insert(hook.name.clone(), result);
                }
            }
        }

        // Snapshot callbacks and tracker for use in spawned tasks
        let model_call_fn = self.get_model_call_fn();
        let agent_fn = self.get_agent_fn();
        let sdk_callback_fn = self.get_sdk_callback_fn();
        let async_tracker = lock_read(&self.async_tracker, "async_tracker").clone();

        // Stable-sort matched hooks by handler type for deterministic result aggregation.
        // Command(0) > Webhook(1) > Prompt(2) > Agent(3) > Inline(4) > SdkCallback(5)
        let mut matching = matching;
        matching.sort_by_key(|h| match &h.handler {
            HookHandler::Command { .. } => 0,
            HookHandler::Webhook { .. } => 1,
            HookHandler::Prompt { .. } => 2,
            HookHandler::Agent { .. } => 3,
            HookHandler::Inline => 4,
            HookHandler::SdkCallback { .. } => 5,
        });

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
                let sdk_callback_fn = sdk_callback_fn.clone();
                // Config-based async: background the hook immediately unless forced sync
                // or SessionStart (always sync).
                let should_background = hook.is_async
                    && !hook.force_sync_execution
                    && ctx.event_type != HookEventType::SessionStart;
                // Use pre-computed inline result if available
                let inline_result = inline_results.remove(&hook_name);
                let async_tracker = async_tracker.clone();
                async move {
                    // If config-based async, return Async immediately
                    if should_background {
                        let task_id = format!("async-{}", uuid::Uuid::new_v4());
                        info!(
                            hook_name = %hook_name,
                            task_id = %task_id,
                            "Backgrounding config-async hook"
                        );

                        // Spawn the actual execution in a background task
                        let bg_hook_name = hook_name.clone();
                        let bg_task_id = task_id.clone();
                        let bg_tracker = async_tracker.clone();
                        tokio::spawn(async move {
                            let start = Instant::now();
                            let timeout = tokio::time::Duration::from_secs(timeout_secs as u64);
                            let result = tokio::time::timeout(
                                timeout,
                                execute_handler(
                                    &handler,
                                    &ctx,
                                    model_call_fn.as_ref(),
                                    agent_fn.as_ref(),
                                    sdk_callback_fn.as_ref(),
                                ),
                            )
                            .await;

                            let duration_ms = start.elapsed().as_millis() as i64;
                            match result {
                                Ok((r, _suppress)) => {
                                    info!(
                                        hook_name = %bg_hook_name,
                                        task_id = %bg_task_id,
                                        duration_ms,
                                        "Background hook completed"
                                    );
                                    // Deliver result to async tracker for system reminder delivery
                                    if let Some(tracker) = bg_tracker {
                                        tracker.complete(&bg_task_id, r);
                                    }
                                }
                                Err(_) => {
                                    warn!(
                                        hook_name = %bg_hook_name,
                                        task_id = %bg_task_id,
                                        timeout_secs,
                                        "Background hook timed out"
                                    );
                                }
                            }
                        });

                        return (
                            HookOutcome {
                                hook_name: hook_name.clone(),
                                result: HookResult::Async {
                                    task_id,
                                    hook_name: hook_name.clone(),
                                },
                                duration_ms: 0,
                                suppress_output: false,
                            },
                            false, // Don't remove one-shot hooks for async
                            hook_name,
                        );
                    }

                    let start = Instant::now();
                    let timeout = tokio::time::Duration::from_secs(timeout_secs as u64);
                    let result = if let Some(r) = inline_result {
                        // Inline handler already executed synchronously
                        Ok((r, false))
                    } else {
                        tokio::time::timeout(
                            timeout,
                            execute_handler(
                                &handler,
                                &ctx,
                                model_call_fn.as_ref(),
                                agent_fn.as_ref(),
                                sdk_callback_fn.as_ref(),
                            ),
                        )
                        .await
                    };

                    let duration_ms = start.elapsed().as_millis() as i64;

                    let (result, is_success, suppress_output) = match result {
                        Ok((r, suppress)) => {
                            let success = !matches!(r, HookResult::Reject { .. });
                            (r, success, suppress)
                        }
                        Err(_) => {
                            warn!(
                                hook_name = %hook_name,
                                timeout_secs,
                                "Hook timed out"
                            );
                            (HookResult::Continue, false, false)
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
                            suppress_output,
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
        let mut hooks = lock_write(&self.hooks, "hooks");
        let before = hooks.len();
        hooks.retain(|h| !(h.once && names_set.contains(&h.name)));
        let removed = before - hooks.len();
        if removed > 0 {
            info!(
                count = removed,
                "Removed one-shot hooks after successful execution"
            );
        }
        drop(hooks);
        let mut handlers = lock_write(&self.inline_handlers, "inline_handlers");
        for name in names {
            handlers.remove(name);
        }
    }

    /// Removes all hooks from a specific source (e.g., when a skill ends).
    pub fn remove_hooks_by_source_name(&self, source_name: &str) {
        let mut removed_names = Vec::new();
        let mut hooks = lock_write(&self.hooks, "hooks");
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
        drop(hooks);
        if !removed_names.is_empty() {
            let mut handlers = lock_write(&self.inline_handlers, "inline_handlers");
            for name in &removed_names {
                handlers.remove(name);
            }
        }
    }

    /// Removes all hooks with the specified scope.
    pub fn remove_hooks_by_scope(&self, scope: crate::scope::HookScope) {
        let mut hooks = lock_write(&self.hooks, "hooks");
        let before = hooks.len();
        hooks.retain(|h| h.source.scope() != scope);
        let removed = before - hooks.len();
        if removed > 0 {
            info!(scope = %scope, count = removed, "Removed hooks by scope");
        }
    }

    /// Removes all registered hooks.
    pub fn clear(&self) {
        lock_write(&self.hooks, "hooks").clear();
        lock_write(&self.inline_handlers, "inline_handlers").clear();
    }

    /// Returns the number of registered hooks.
    pub fn len(&self) -> usize {
        lock_read(&self.hooks, "hooks").len()
    }

    /// Returns `true` if no hooks are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a copy of all registered hooks.
    pub fn all_hooks(&self) -> Vec<HookDefinition> {
        lock_read(&self.hooks, "hooks").clone()
    }

    /// Registers a group of hooks with a shared group ID.
    ///
    /// Used by subagent hooks: all hooks in the group share the same
    /// `group_id` so they can be unregistered together when the agent completes.
    pub fn register_group(&self, group_id: &str, hooks: impl IntoIterator<Item = HookDefinition>) {
        let mut all = lock_write(&self.hooks, "hooks");
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

    /// Removes all hooks with the given group ID.
    ///
    /// Used to clean up subagent hooks when the agent completes.
    pub fn unregister_group(&self, group_id: &str) {
        let mut hooks = lock_write(&self.hooks, "hooks");
        let before = hooks.len();
        hooks.retain(|h| h.group_id.as_deref() != Some(group_id));
        let removed = before - hooks.len();
        if removed > 0 {
            info!(group_id, count = removed, "Removed hooks by group");
        }
    }
}

/// Dispatches execution to the appropriate handler.
///
/// Returns `(result, suppress_output)`. The `suppress_output` flag is only set
/// by Command and Webhook handlers (which can parse `HookOutput.suppressOutput`).
async fn execute_handler(
    handler: &HookHandler,
    ctx: &HookContext,
    model_call_fn: Option<&HookModelCallFn>,
    agent_fn: Option<&HookAgentFn>,
    sdk_callback_fn: Option<&HookSdkCallbackFn>,
) -> (HookResult, bool) {
    match handler {
        HookHandler::Command { command } => {
            // Pass full HookContext to command handler for env vars and stdin JSON
            handlers::command::CommandHandler::execute(command, ctx).await
        }
        HookHandler::Prompt { template, model } => {
            let result = if model.is_some() {
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
            };
            (result, false)
        }
        HookHandler::Agent {
            max_turns, prompt, ..
        } => {
            let result = if let Some(spawn_fn) = agent_fn {
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
            };
            (result, false)
        }
        HookHandler::Webhook {
            url,
            timeout,
            headers,
        } => {
            handlers::webhook::WebhookHandler::execute_with_options(
                url,
                ctx,
                timeout.map(|t| t as u64),
                headers,
            )
            .await
        }
        HookHandler::Inline => {
            // Inline handlers are pre-executed synchronously in execute().
            // If we reach here, no inline handler was registered for this hook.
            warn!("Inline hook definition has no registered handler closure");
            (HookResult::Continue, false)
        }
        HookHandler::SdkCallback { callback_id } => {
            if let Some(callback_fn) = sdk_callback_fn {
                let event_type = format!("{:?}", ctx.event_type);
                let input = serde_json::to_value(ctx).unwrap_or(serde_json::Value::Null);
                match callback_fn(callback_id.clone(), event_type, input).await {
                    Ok(output) => parse_sdk_callback_response(output),
                    Err(e) => {
                        warn!(callback_id, "SDK hook callback failed: {e}");
                        (HookResult::Continue, false)
                    }
                }
            } else {
                warn!(
                    callback_id,
                    "SdkCallback hook has no registered callback function"
                );
                (HookResult::Continue, false)
            }
        }
    }
}

/// Parse an SDK callback response into a `HookResult`.
///
/// Tries to deserialize as `HookResult` first, then falls back to
/// `HookOutput` (Claude Code v2.1.7 format), and finally defaults
/// to `Continue`.
fn parse_sdk_callback_response(output: serde_json::Value) -> (HookResult, bool) {
    // Try direct HookResult deserialization
    if let Ok(result) = serde_json::from_value::<HookResult>(output.clone()) {
        return (result, false);
    }

    // Try HookOutput format (has continue_execution, updated_input, etc.)
    if let Ok(hook_output) = serde_json::from_value::<handlers::command::HookOutput>(output) {
        let suppress = hook_output.suppress_output;
        return (hook_output.into_result(None), suppress);
    }

    // Unrecognized format — continue by default
    (HookResult::Continue, false)
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
