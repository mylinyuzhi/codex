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
//! prepended. Cache-sharing callers such as promptSuggestion must pass
//! their prompt as the fork user message and leave this override unset.
//!
//! ## What this is NOT
//!
//! It is not a generic "run another query" helper — it specifically
//! implements the post-turn cache-sharing contract. AgentTool spawn
//! goes through [`coco_query::QueryEngineAdapter`] (different
//! contract: no cache slot, full child engine lifecycle).

use std::sync::Arc;

use coco_query::QueryEngineConfig;
use coco_query::forked_agent::{ForkDispatcher, ForkedAgentOptions, ForkedAgentResult};
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
    ) -> Result<ForkedAgentResult, coco_error::BoxedError> {
        // PR #18143 cache-bust trail. Setting max_output_tokens on a
        // cache-shared fork clamps `budget_tokens`, invalidating the
        // parent's prompt cache (TS incident: 92.7% → 61% hit-rate).
        // Only `Compact` legitimately sets this (distinct model).
        // Surface a structured warn! on every other variant so a
        // regression leaves a trail in the logs.
        if let Some(cap) = options.max_output_tokens
            && !matches!(options.fork_label, coco_types::ForkLabel::Compact)
        {
            tracing::warn!(
                fork_label = %options.fork_label,
                max_output_tokens = cap,
                "max_output_tokens override set on cache-shared fork; cache key parity broken (PR #18143: 92.7% → 61% hit-rate incident)"
            );
        }

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

        // Forks inherit the parent's settings-driven permission rules;
        // re-resolve from the same layered settings the parent uses.
        let (allow_rules, deny_rules, ask_rules) =
            crate::permission_rule_loader::typed_permission_rules(&runtime_config.settings);

        let engine_config = QueryEngineConfig {
            model_id: agent_config.model.clone(),
            permission_mode: coco_types::PermissionMode::Default,
            allow_rules,
            deny_rules,
            ask_rules,
            context_window: agent_config.context_window.unwrap_or(200_000),
            max_output_tokens: agent_config.max_output_tokens.unwrap_or(16_384),
            max_turns: agent_config.max_turns.unwrap_or(1),
            max_tokens: None,
            prompt_cache: agent_config.prompt_cache.clone(),
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
            lsp_config: runtime_config.lsp.clone(),
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
            // Per-fork plumbing — thread the canUseTool callback,
            // fork_label, query_source override, and (cache-bust-risky)
            // max_output_tokens override onto the child engine config
            // so step 3.5 in execute_tool_call enforces uniformly and
            // log lines self-identify which fork they belong to.
            can_use_tool: options.can_use_tool.clone(),
            query_source_override: Some(options.query_source.clone()),
            fork_label: Some(options.fork_label),
            max_output_tokens_override: options.max_output_tokens,
            // Sub-context isolation primitives applied at the
            // per-call ToolUseContext build site (tool_context.rs
            // reads `fork_isolation` and applies auto agent_id,
            // fresh denial tracking, query_chain_id / query_depth
            // bump, allowed_write_roots fence, and require_can_use_tool).
            // TS parity: `forkedAgent.ts::createSubagentContext`.
            fork_isolation: Some(Arc::new({
                let mut iso =
                    coco_query::fork_context::ForkContextOverrides::for_label(options.fork_label);
                iso.query_source = options.query_source.clone();
                iso.can_use_tool = options.can_use_tool.clone();
                iso.require_can_use_tool = options.require_can_use_tool;
                iso
            })),
            ..Default::default()
        };

        // Build a fresh engine via the runtime's standard wiring.
        // `wire_engine` installs every per-session subsystem — the fork
        // gets the same hooks / observers / mailbox / agent handle the
        // parent has, which keeps event emission / permission gating
        // consistent across the parent and child.
        //
        // Cancellation: forks are short-lived; honor the caller's
        // override (speculation / compact share parent's abort token
        // so user `Esc` aborts the fork) — fall back to a fresh
        // independent token when the caller didn't supply one.
        let cancel = options.overrides.abort.clone().unwrap_or_default();
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

        // Multi-message capture (TS parity:
        // `utils/forkedAgent.ts::runForkedAgent` returns the engine's
        // actual `Vec<Message>`). The fork's full assistant +
        // user-tool-result sequence lands here so callers
        // (`promptSuggestion::extract_suggestion_text`) can walk
        // "tool→denied→text" turn-2 fallback paths correctly.
        //
        // `QueryResult.final_messages` carries every assistant +
        // tool-result message produced during the fork's run; we
        // strip the parent-history prefix that the engine prepended
        // so the caller only sees the fork's own emissions (TS
        // `runForkedAgent` likewise returns just the fork's added
        // messages, not the parent's).
        let parent_msg_count = agent_config.fork_context_messages.len();
        let fork_messages: Vec<coco_messages::Message> = result
            .final_messages
            .iter()
            .skip(parent_msg_count + 1) // +1 for the user prompt the fork prepended
            .cloned()
            .collect();

        Ok(ForkedAgentResult {
            messages: fork_messages,
            total_usage: result.total_usage,
            stop_reason: result.stop_reason,
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
