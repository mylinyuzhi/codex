//! Bridge: `AgentQueryEngine` (tool-runtime) → `AgentExecutionEngine`
//! (coordinator's runner-loop trait).
//!
//! TS: `inProcessRunner.ts` calls `query()` directly — no abstraction
//! needed because TS doesn't have the layer-strict trait split. Rust
//! has both an `AgentQueryEngine` (subagent path, owned by `coco-query`)
//! and an `AgentExecutionEngine` (teammate runner-loop path, owned by
//! `coco-coordinator`). This bridge lets the same engine drive both.
//!
//! Lives inside `coco-coordinator` so SwarmAgentHandle can install the
//! same `Arc<dyn AgentQueryEngine>` it already holds for subagent
//! spawns and have it satisfy the teammate-loop contract.

use std::sync::Arc;

use async_trait::async_trait;

use crate::runner_loop::AgentExecutionEngine;
use crate::runner_loop::AgentQueryConfig as RunnerAgentQueryConfig;
use crate::runner_loop::AgentQueryResult as RunnerAgentQueryResult;

/// Adapter that lets a `coco_tool_runtime::AgentQueryEngine` drive the
/// in-process teammate loop via the coordinator's
/// [`AgentExecutionEngine`] trait.
pub struct TeammateExecutionAdapter {
    inner: coco_tool_runtime::AgentQueryEngineRef,
}

impl TeammateExecutionAdapter {
    pub fn new(inner: coco_tool_runtime::AgentQueryEngineRef) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl AgentExecutionEngine for TeammateExecutionAdapter {
    async fn run_query(
        &self,
        prompt: &str,
        config: RunnerAgentQueryConfig,
    ) -> crate::Result<RunnerAgentQueryResult> {
        let tool_runtime_config = coco_tool_runtime::AgentQueryConfig {
            system_prompt: config.system_prompt,
            model: config.model.unwrap_or_default(),
            max_turns: config.max_turns,
            allowed_tools: config.allowed_tools,
            disallowed_tools: config.disallowed_tools,
            extra_permission_rules: config.extra_permission_rules,
            live_permission_rules: config.live_permission_rules,
            live_permission_mode: config.live_permission_mode,
            tool_overrides: config.tool_overrides,
            features: config.features,
            parent_tool_filter: config.parent_tool_filter,
            preserve_tool_use_results: config.preserve_tool_use_results,
            permission_mode: config.permission_mode,
            cancel: config.cancel,
            bypass_permissions_available: config.bypass_permissions_available,
            fork_context_messages: config.fork_context_messages,
            is_teammate: true,
            is_in_process_teammate: true,
            effort: config.effort,
            use_exact_tools: config.use_exact_tools,
            mcp_servers: config.mcp_servers,
            model_role: config.model_role,
            model_selection: config.model_selection,
            ..Default::default()
        };

        let result = self
            .inner
            .execute_query(prompt, tool_runtime_config)
            .await
            .map_err(|e| crate::CoordinatorError::generic(format!("{e}")))?;

        Ok(RunnerAgentQueryResult {
            messages: result.messages,
            token_count: result.input_tokens + result.output_tokens,
            input_tokens: result.input_tokens,
            output_tokens: result.output_tokens,
            turns: result.turns,
            tool_use_count: result.tool_use_count as i32,
            cancelled: result.cancelled,
            response_text: result.response_text,
        })
    }

    /// Full LLM compact for teammate history.
    ///
    /// Pipeline (TS parity: `inProcessRunner.ts:1090`
    /// `compactConversation`):
    /// 1. Apply micro-compact first to drop resolved tool-result
    ///    content (mirrors `compact.ts:98` running microcompact
    ///    pre-summarization).
    /// 2. Run `coco_compact::compact_conversation` with our own
    ///    summarize callback that issues a no-tools query through
    ///    the wrapped `AgentQueryEngine` and returns the response
    ///    text as the summary body.
    /// 3. Build post-compact messages (boundary marker + summary
    ///    user message + kept recent rounds) and serialise back to
    ///    `Vec<serde_json::Value>` for the runner-loop.
    ///
    /// On any error (deserialisation / engine failure / compact
    /// failure) returns the input unchanged so the runner-loop's
    /// sliding-window safety valve still bounds growth.
    async fn compact_messages(
        &self,
        messages: Vec<serde_json::Value>,
        _total_tokens: i64,
    ) -> crate::Result<Vec<serde_json::Value>> {
        const KEEP_RECENT_FOR_MICRO: usize = 5;
        const KEEP_RECENT_ROUNDS_FOR_FULL: usize = 2;

        let mut typed: Vec<coco_messages::Message> = match messages
            .iter()
            .map(|v| serde_json::from_value::<coco_messages::Message>(v.clone()))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "teammate compact_messages: malformed history; returning input unchanged"
                );
                return Ok(messages);
            }
        };

        // Step 1 — micro-compact (no-LLM cleanup of resolved tool
        // results). Cheap and always safe; bounds the prompt fed
        // into the summarizer.
        let _ = coco_compact::micro_compact(&mut typed, KEEP_RECENT_FOR_MICRO);

        // Step 2 — full LLM compact via summarize callback that
        // routes through the wrapped engine. Building a fresh
        // closure here per call keeps the borrow simple — the
        // engine ref is `Arc<dyn>` so cloning is cheap.
        let engine = self.inner.clone();
        let summarize = move |attempt: coco_compact::CompactSummaryAttempt| {
            let engine = engine.clone();
            async move {
                let cfg = coco_tool_runtime::AgentQueryConfig {
                    system_prompt: String::new(),
                    model: String::new(),
                    max_turns: Some(1),
                    max_output_tokens: Some(attempt.max_summary_tokens),
                    fork_context_messages: attempt
                        .context_messages
                        .iter()
                        .filter_map(|m| serde_json::to_value(m).ok())
                        .collect(),
                    // Deliberately impossible allow-list: no tools
                    // during the summarization turn. The prompt itself
                    // is separate from structured fork context so we do
                    // not re-flatten history into a legacy string.
                    allowed_tools: vec![String::new()],
                    is_teammate: true,
                    is_in_process_teammate: true,
                    ..Default::default()
                };
                match engine.execute_query(&attempt.summary_request, cfg).await {
                    Ok(result) => Ok(coco_compact::CompactSummaryResponse {
                        summary: result.response_text.unwrap_or_default(),
                    }),
                    Err(e) => Err(e.to_string()),
                }
            }
        };

        let opts = coco_compact::CompactRunOptions {
            keep_recent_rounds: KEEP_RECENT_ROUNDS_FOR_FULL,
            ..Default::default()
        };

        let compact_result =
            match coco_compact::compact_conversation(&typed, &opts, summarize, None).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "teammate full LLM compact failed; falling back to micro-compact only"
                    );
                    // Return the micro-compacted history rather than
                    // the original — the micro pass still trimmed
                    // tool results which buys some bound.
                    return Ok(typed
                        .iter()
                        .map(|m| serde_json::to_value(m).unwrap_or_default())
                        .collect());
                }
            };

        // Step 3 — assemble post-compact messages and serialise.
        let post_compact = coco_compact::build_post_compact_messages(&compact_result);
        Ok(post_compact
            .iter()
            .map(|m| serde_json::to_value(m).unwrap_or_default())
            .collect())
    }
}

/// Convenience: wrap an `AgentQueryEngineRef` for the runner-loop side.
pub fn into_execution_engine(
    inner: coco_tool_runtime::AgentQueryEngineRef,
) -> Arc<dyn AgentExecutionEngine> {
    Arc::new(TeammateExecutionAdapter::new(inner))
}
