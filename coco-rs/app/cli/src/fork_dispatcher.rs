//! Production [`ForkDispatcher`] backed by [`SessionRuntime`].
//!
//! D1 / D2: post-turn fork callers (`/btw`, `promptSuggestion`,
//! and future `postTurnSummary` / `extractMemories` paths) need to
//! drive a *fresh* [`coco_query::QueryEngine`] without mutating the
//! parent. This module owns that bridge.
//!
//! TS reference: `utils/forkedAgent.ts::runForkedAgent` —
//! constructs an `AgentQueryConfig` from `lastCacheSafeParams`,
//! runs a one-shot turn against a fresh engine, returns the
//! response text. coco-rs threads the same contract through the
//! [`coco_query::forked_agent::ForkDispatcher`] trait so that
//! `app/query` stays free of CLI / runtime types.
//!
//! ## Cache parity
//!
//! The dispatcher reuses [`coco_query::forked_agent::build_query_config`]
//! to derive a config that matches the parent's prompt-cache key
//! (system prompt bytes, model id, fork-context messages). When
//! callers pass `system_prompt_override`, the override replaces
//! `cache.rendered_system_prompt` *before* the parent history is
//! prepended — the parent cache is intentionally invalidated for
//! the override case (e.g. promptSuggestion's bespoke system
//! prompt) but the rest of the request shape stays put.
//!
//! ## What this is NOT
//!
//! It is not a generic "run another query" helper — it specifically
//! implements the post-turn cache-sharing contract. AgentTool spawn
//! goes through [`coco_query::QueryEngineAdapter`] (different
//! contract: no cache slot, full child engine lifecycle).

use std::sync::Arc;

use coco_query::QueryEngineConfig;
use coco_query::forked_agent::{ForkDispatcher, ForkedAgentOptions, ForkedDispatchResult};
use coco_types::CacheSafeParams;

use crate::session_runtime::SessionRuntime;

/// Backed by `Arc<SessionRuntime>` — captures it once, reuses for
/// every dispatch. Cheap to construct; cheap to call.
pub struct SessionRuntimeForkDispatcher {
    runtime: Arc<SessionRuntime>,
}

impl SessionRuntimeForkDispatcher {
    pub fn new(runtime: Arc<SessionRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait]
impl ForkDispatcher for SessionRuntimeForkDispatcher {
    async fn dispatch(
        &self,
        cache: &CacheSafeParams,
        options: &ForkedAgentOptions,
        prompt: &str,
        system_prompt_override: Option<String>,
    ) -> Result<ForkedDispatchResult, coco_error::BoxedError> {
        // Derive the AgentQueryConfig shape from the cache slot. This
        // keeps the byte-faithful contract documented on `forked_agent`
        // (skip_cache_write, skip_transcript, max_turns: 1 by default).
        let mut agent_config = coco_query::forked_agent::build_query_config(cache, options);
        if let Some(system) = system_prompt_override {
            agent_config.system_prompt = system;
        }

        // Resolve the parent runtime config. The fork inherits the
        // parent's tool/sandbox/web_*/feature/role configuration so
        // the child engine sees the same world the parent does — TS
        // parity: forks share `toolUseContext` with the parent.
        let runtime_config = self.runtime.runtime_config.as_ref();

        let engine_config = QueryEngineConfig {
            model_id: agent_config.model.clone(),
            permission_mode: coco_types::PermissionMode::Default,
            context_window: agent_config.context_window.unwrap_or(200_000),
            max_output_tokens: agent_config.max_output_tokens.unwrap_or(16_384),
            max_turns: agent_config.max_turns.unwrap_or(1),
            max_tokens: None,
            system_prompt: Some(agent_config.system_prompt.clone()),
            streaming_tool_execution: false,
            session_id: agent_config.session_id.clone().unwrap_or_default(),
            tool_config: runtime_config.tool.clone(),
            sandbox_config: runtime_config.sandbox.clone(),
            sandbox_state: self.runtime.sandbox_state(),
            memory_config: runtime_config.memory.clone(),
            shell_config: runtime_config.shell.clone(),
            web_fetch_config: runtime_config.web_fetch.clone(),
            web_search_config: runtime_config.web_search.clone(),
            compact: runtime_config.compact.clone(),
            features: Arc::new(runtime_config.features.clone()),
            tool_overrides: runtime_config.tool_overrides.clone(),
            is_non_interactive: true,
            // Fork dispatch is fire-and-forget — model-driven thinking
            // / effort overrides would invalidate the parent cache, so
            // we skip them. `forked_agent::build_query_config` already
            // honors `options.effort` when the caller wants it; we
            // forward that through the engine config below.
            thinking_level: agent_config
                .effort
                .as_deref()
                .and_then(|s| s.parse::<coco_types::ThinkingLevel>().ok()),
            ..Default::default()
        };

        // Build a fresh engine via the runtime's standard wiring.
        // `wire_engine` installs every per-session subsystem — the fork
        // gets the same hooks / observers / mailbox / agent handle the
        // parent has, which keeps event emission / permission gating
        // consistent across the parent and child.
        //
        // Cancellation: forks are short-lived; we hand them an
        // independent token rather than threading the parent's, so a
        // cancel of the parent loop doesn't tear down a fork mid-flight
        // (and vice versa).
        let cancel = tokio_util::sync::CancellationToken::new();
        let engine = self
            .runtime
            .build_engine_from_config(engine_config, cancel, None)
            .await;

        // Drive the engine. `fork_context_messages` carries the
        // parent's history verbatim, mirroring the cache-share path.
        // Empty fork-context messages → run with the prompt only
        // (rare; promptSuggestion etc. always pass parent history).
        let result = if !agent_config.fork_context_messages.is_empty() {
            let mut messages: Vec<coco_messages::Message> = Vec::new();
            for v in &agent_config.fork_context_messages {
                if let Ok(m) = serde_json::from_value::<coco_messages::Message>(v.clone()) {
                    messages.push(m);
                }
            }
            messages.push(coco_messages::create_user_message(prompt));
            // Discard event stream — fork output goes back via the
            // returned text, not via the parent's CoreEvent channel.
            let (tx, _rx) = tokio::sync::mpsc::channel(8);
            engine.run_with_messages(messages, tx).await.map_err(|e| {
                Box::new(coco_error::PlainError::new(
                    format!("fork engine run_with_messages: {e}"),
                    coco_error::StatusCode::Internal,
                )) as coco_error::BoxedError
            })?
        } else {
            engine.run(prompt).await.map_err(|e| {
                Box::new(coco_error::PlainError::new(
                    format!("fork engine run: {e}"),
                    coco_error::StatusCode::Internal,
                )) as coco_error::BoxedError
            })?
        };

        Ok(ForkedDispatchResult {
            text: result.response_text,
            input_tokens: result.total_usage.input_tokens,
            output_tokens: result.total_usage.output_tokens,
        })
    }
}

/// Convenience: install a [`SessionRuntimeForkDispatcher`] onto
/// `runtime` post-`build()`. Idempotent — calling twice replaces
/// the previous installation.
pub async fn install(runtime: Arc<SessionRuntime>) {
    let dispatcher: coco_query::forked_agent::ForkDispatcherRef =
        Arc::new(SessionRuntimeForkDispatcher::new(runtime.clone()));
    runtime.attach_fork_dispatcher(dispatcher).await;
}
