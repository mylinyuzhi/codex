//! Generic post-turn / side-channel forked-agent helper.
//!
//! TS: `utils/forkedAgent.ts::runForkedAgent` + `utils/sideQuestion.ts`.
//!
//! Six TS callers consume the parent session's `lastCacheSafeParams` to
//! run a one-shot side-channel query that shares the parent's prompt
//! cache: `/btw`, `promptSuggestion`, `postTurnSummary`, `extractMemories`,
//! `autoDream`, `AgentSummary`, `sessionMemory`. They all follow the
//! same pattern:
//!
//! 1. Read [`coco_types::CacheSafeParams`] from the post-turn slot
//!    (`QueryEngine::last_cache_safe_params`).
//! 2. Build a one-shot `AgentQueryConfig` with `max_turns: 1`,
//!    `skip_cache_write: true`, deny-all permissions (or the caller's
//!    chosen `canUseTool` policy).
//! 3. Drive a fresh `QueryEngine` with the cached params + new prompt.
//!
//! This module owns step 2 — the shared scaffolding — so each caller
//! becomes a thin entry-point that produces a prompt and consumes a
//! result.
//!
//! ## Cache-parity contract
//!
//! [`ForkedAgentOptions::default`] returns the conservative shape that
//! preserves the parent's prompt cache:
//!
//! - `max_turns: Some(1)` — single round-trip
//! - `skip_transcript: true` — no sidechain noise in the parent's
//!   transcript
//! - `skip_cache_write: true` — fire-and-forget; don't pollute the
//!   shared cache with this branch
//! - `effort: None` — leaves thinking config untouched (TS PR #18143
//!   incident: setting `effort: 'low'` on prompt-suggestion forks
//!   collapsed cache hit rate from 92.7% → 61% by changing
//!   `budget_tokens` and busting the cache key)
//!
//! Override these only when cache parity isn't a goal (e.g. compact
//! summaries that intentionally use a different model / budget).

use std::sync::Arc;

use coco_tool_runtime::AgentQueryConfig;
use coco_types::CacheSafeParams;

/// Runtime knobs for a forked-agent invocation.
#[derive(Debug, Clone)]
pub struct ForkedAgentOptions {
    /// Hard cap on turns. `Some(1)` is the standard "one-shot" shape.
    pub max_turns: Option<i32>,
    /// `true` ⇒ fork's history doesn't enter the parent's transcript
    /// store. Default for ephemeral / fire-and-forget side queries.
    pub skip_transcript: bool,
    /// `true` ⇒ the fork's API request asks the provider not to
    /// write a fresh prompt-cache entry on the last message.
    pub skip_cache_write: bool,
    /// Optional reasoning-effort override. **Setting this busts cache
    /// parity** for older models that don't have adaptive thinking.
    /// Default `None` preserves the parent's cache key.
    pub effort: Option<String>,
    /// Identifier surfaced in telemetry / logs so cache-break
    /// attribution and hit-rate dashboards can split this fork's
    /// traffic from the parent loop.
    pub query_source: String,
}

impl Default for ForkedAgentOptions {
    fn default() -> Self {
        Self {
            max_turns: Some(1),
            skip_transcript: true,
            skip_cache_write: true,
            effort: None,
            query_source: "fork".into(),
        }
    }
}

/// Convenience: default options with a labelled `query_source`.
pub fn one_shot_options(query_source: impl Into<String>) -> ForkedAgentOptions {
    ForkedAgentOptions {
        query_source: query_source.into(),
        ..ForkedAgentOptions::default()
    }
}

/// Build a one-shot `AgentQueryConfig` from cached parent params +
/// caller-provided options. The result is suitable for handing to a
/// fresh engine that shares the parent's prompt cache (subject to the
/// [`ForkedAgentOptions`] cache-parity rules).
///
/// The returned config's `model` matches the parent's, `system_prompt`
/// matches the parent's pre-rendered bytes, and
/// `fork_context_messages` carries the parent's serialized post-turn
/// history. The caller is responsible for invoking the engine — this
/// helper just standardises the config shape so all six post-turn
/// fork callers don't drift.
pub fn build_query_config(
    cache: &CacheSafeParams,
    options: &ForkedAgentOptions,
) -> AgentQueryConfig {
    AgentQueryConfig {
        system_prompt: cache.rendered_system_prompt.clone(),
        model: cache.model_id.clone(),
        max_turns: options.max_turns,
        // Inherit the parent's history verbatim so the API request's
        // prefix bytes match — this is what enables cache sharing.
        fork_context_messages: cache.fork_context_messages.clone(),
        allowed_tools: Vec::new(),
        disallowed_tools: Vec::new(),
        effort: options.effort.clone(),
        ..Default::default()
    }
}

/// Adapter type the engine calls to ask "may this tool run?". Forked
/// callers pass [`deny_all`] for fire-and-forget queries.
pub type ForkedCanUseTool = Arc<dyn Fn(&str) -> CanUseToolDecision + Send + Sync>;

/// Decision returned by a [`ForkedCanUseTool`] callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanUseToolDecision {
    Allow,
    Deny,
}

/// Cache-parity-safe default: deny every tool. Used by `/btw`,
/// promptSuggestion, postTurnSummary — the side-channel queries that
/// only want a text answer from the model and shouldn't be running
/// any tools.
pub fn deny_all() -> ForkedCanUseTool {
    Arc::new(|_| CanUseToolDecision::Deny)
}

/// Result of a [`ForkDispatcher::dispatch`] call.
///
/// `text` is the model's textual response (assistant turn) — already
/// extracted, joined, and stripped of any tool blocks. The two
/// numeric fields are surfaced for telemetry callers; callers that
/// only want the answer can ignore them.
#[derive(Debug, Clone)]
pub struct ForkedDispatchResult {
    pub text: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// Async trait for dispatching a one-shot forked query.
///
/// Implementations capture whatever they need to build a fresh
/// [`coco_query::QueryEngine`] (typically `Arc<SessionRuntime>` in the
/// CLI) and drive a single turn against it. The parent engine's
/// history is *not* mutated — that's the whole point of forking.
///
/// TS reference: `utils/forkedAgent.ts::runForkedAgent`. Six call
/// sites (`/btw`, promptSuggestion, postTurnSummary, extractMemories,
/// autoDream, AgentSummary) all route through the same TS function;
/// the trait gives Rust callers the same single seam.
#[async_trait::async_trait]
pub trait ForkDispatcher: Send + Sync {
    /// Run a forked query.
    ///
    /// `cache` is the parent's [`CacheSafeParams`] (typically read
    /// from `QueryEngine::last_cache_safe_params`). `prompt` is the
    /// new user message to append after the cached parent history.
    /// `system_prompt_override` lets callers (notably
    /// `promptSuggestion`) substitute a different system prompt
    /// while keeping the rest of the cache parity intact — when
    /// `None`, `cache.rendered_system_prompt` is used.
    async fn dispatch(
        &self,
        cache: &CacheSafeParams,
        options: &ForkedAgentOptions,
        prompt: &str,
        system_prompt_override: Option<String>,
    ) -> anyhow::Result<ForkedDispatchResult>;
}

/// Convenience reference type matching the rest of the engine's
/// trait-object slots.
pub type ForkDispatcherRef = Arc<dyn ForkDispatcher>;

#[cfg(test)]
#[path = "forked_agent.test.rs"]
mod tests;
