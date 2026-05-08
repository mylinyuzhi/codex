//! Callback handle for LLM-driven hook handlers.
//!
//! `Prompt` and `Agent` hook handlers need an LLM. The hooks crate sits
//! at L4 in the dependency graph and cannot reach `coco-inference`
//! directly without violating layer rules. Callers (typically `coco-cli`
//! / `coco-query`) implement [`HookLlmHandle`] over their `ApiClient`
//! and install it on [`crate::orchestration::OrchestrationContext`].
//!
//! TS source:
//! - `utils/hooks/execPromptHook.ts:21-211` — single-turn `queryModelWithoutStreaming`
//! - `utils/hooks/execAgentHook.ts:36-339` — multi-turn `query()` with
//!   `MAX_AGENT_TURNS=50` and `StructuredOutputTool` enforcement.
//!
//! The trait deliberately does not return `Vec<Message>` or other
//! provider-shaped data — hooks only care about the `{ok, reason}`
//! structured output produced by both code paths.
//!
//! # Layering
//!
//! ```text
//! coco-hooks (L4) defines HookLlmHandle (this file)
//!     ↓ Arc<dyn HookLlmHandle>
//! OrchestrationContext.llm_handle
//!     ↓ used by
//! execute_hooks_parallel_filtered spawn loop
//!     ↓
//! HookHandler::Prompt / HookHandler::Agent → handle.evaluate_*()
//! ```
//!
//! Implementations live in `coco-query` (sees `ApiClient`), wired by
//! `coco-cli::session_runtime`.

use std::time::Duration;

/// Outcome of evaluating a `Prompt` or `Agent` hook through an LLM.
///
/// Maps onto the `{ok: bool, reason?: string}` schema in
/// `hookHelpers.ts:hookResponseSchema`.
#[derive(Debug, Clone)]
pub enum HookEvaluationResult {
    /// `ok: true` — condition met. Treated as `HookOutcome::Success`.
    Ok,
    /// `ok: false` — condition not met. `reason` flows into a
    /// `blocking_error` that surfaces as `<hook-blocking-error>` to the
    /// model.
    Blocking { reason: String },
    /// Hit `MAX_AGENT_TURNS` (agent only) or finished without
    /// `StructuredOutputTool`. TS treats this as `'cancelled'` —
    /// silent, no UI message.
    Cancelled,
    /// LLM call failed, schema validation failed, JSON parse failed,
    /// or the timeout fired. Becomes a `hook_non_blocking_error`
    /// attachment so the user sees the failure but the conversation
    /// continues.
    NonBlockingError { error: String },
}

/// Handle that evaluates `Prompt` / `Agent` hooks through the parent
/// session's `ApiClient`. Implemented in `coco-query`; wired via
/// [`crate::orchestration::OrchestrationContext::llm_handle`].
#[async_trait::async_trait]
pub trait HookLlmHandle: Send + Sync + std::fmt::Debug {
    /// One-shot model evaluation matching TS `execPromptHook`.
    ///
    /// `prompt`: the hook's prompt text with `$ARGUMENTS` already
    /// substituted by the caller.
    /// `model`: optional override; `None` falls back to the small/fast
    /// model the implementation chooses.
    /// `timeout`: bound on total wall-clock time; the implementation
    /// is expected to honor it via cancellation.
    async fn evaluate_prompt(
        &self,
        prompt: &str,
        model: Option<&str>,
        timeout: Duration,
    ) -> HookEvaluationResult;

    /// Multi-turn agent evaluation matching TS `execAgentHook`. The
    /// implementation is expected to register a session-level
    /// `StructuredOutputTool` enforcement hook so the agent must call
    /// the tool exactly once before returning. `MAX_AGENT_TURNS=50`
    /// is the TS default; implementations may relax that bound.
    async fn evaluate_agent(
        &self,
        prompt: &str,
        model: Option<&str>,
        timeout: Duration,
    ) -> HookEvaluationResult;
}
