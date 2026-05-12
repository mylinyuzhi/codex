//! `coco-query`'s implementation of [`coco_hooks::HookLlmHandle`].
//!
//! Bridges the `Prompt` and `Agent` hook handler types to the parent
//! session's `ApiClient`. Hooks-crate sits at L4; inference at L2 —
//! the trait lives in `coco-hooks` and is implemented here so the
//! L4 → L2 dependency arrow is reversed.
//!
//! # Status (v1)
//!
//! - **Prompt path**: full implementation. Builds a single-turn
//!   `QueryParams`, calls `ApiClient::query`, parses the assistant
//!   text as `{ok: bool, reason?: string}` JSON. Recursion-safe:
//!   bypasses the `QueryEngine` turn loop entirely so
//!   `UserPromptSubmit` hooks don't fire from within a hook
//!   evaluation. Mirrors TS `execPromptHook.ts:21-211`.
//!
//! - **Agent path**: pragmatic v1 — logs a warning and returns
//!   `Cancelled`. TS `execAgentHook.ts:264` uses the same outcome
//!   when the agent stops without calling `StructuredOutputTool`,
//!   so this is silent (no UI error) and matches TS's worst-case
//!   fallback. Full multi-turn agent evaluation requires:
//!     * `StructuredOutputTool` registered in the tool registry
//!     * A forked `QueryEngine` with `max_turns = 50`
//!     * Session-level "must call StructuredOutput before Stop"
//!       enforcement
//!     * Auto-grant of `Read(/<transcript_path>)` for the run
//!
//!   This is tracked as a P3 follow-up in `crate-coco-hooks.md`.
//!
//! # Model selection
//!
//! TS uses `getSmallFastModel()` (Haiku) by default; the per-hook
//! `hook.model` field can override. Coco-rs has a `ModelRole::HookAgent`
//! variant for the same purpose. v1 uses the main session's
//! `ApiClient` directly — the `model` parameter passed to the trait
//! is logged for telemetry but not yet routed. Per-role ApiClient
//! construction is a P2 follow-up.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use coco_hooks::HookEvaluationResult;
use coco_hooks::HookLlmHandle;
use coco_inference::ApiClient;
use coco_inference::AssistantContentPart;
use coco_inference::LanguageModelMessage;
use coco_inference::QueryParams;
use coco_inference::UserContentPart;
use serde::Deserialize;

/// System prompt prepended to every Prompt hook evaluation.
///
/// Verbatim from TS `execPromptHook.ts:65-70` so model behaviour stays
/// stable across the TS↔Rust port. The schema constraint is enforced
/// by JSON parse rather than a provider-level `output_format` (which
/// is not yet wired in `coco-inference`).
const HOOK_PROMPT_SYSTEM: &str = "You are evaluating a hook in Claude Code.

Your response must be a JSON object matching one of the following schemas:
1. If the condition is met, return: {\"ok\": true}
2. If the condition is not met, return: {\"ok\": false, \"reason\": \"Reason for why it is not met\"}";

/// JSON shape the hook prompt is expected to produce.
#[derive(Debug, Clone, Deserialize)]
struct HookResponse {
    ok: bool,
    #[serde(default)]
    reason: Option<String>,
}

/// `coco-query`'s `HookLlmHandle` implementation. Single struct for
/// both Prompt and Agent paths — they share `client` and the
/// `Cancelled`/`NonBlockingError` mapping logic.
///
/// Manual `Debug` because `ApiClient` itself doesn't derive `Debug`
/// (provider state is non-trivial); we surface only the model id which
/// is what diagnostics actually want.
pub struct QueryHookLlm {
    client: Arc<ApiClient>,
}

impl std::fmt::Debug for QueryHookLlm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryHookLlm")
            .field("model_id", &self.client.model_id())
            .field("provider", &self.client.provider())
            .finish()
    }
}

impl QueryHookLlm {
    pub fn new(client: Arc<ApiClient>) -> Self {
        Self { client }
    }

    pub fn into_handle(self) -> Arc<dyn HookLlmHandle> {
        Arc::new(self) as Arc<dyn HookLlmHandle>
    }
}

#[async_trait]
impl HookLlmHandle for QueryHookLlm {
    async fn evaluate_prompt(
        &self,
        prompt: &str,
        model: Option<&str>,
        timeout: Duration,
    ) -> HookEvaluationResult {
        if let Some(m) = model {
            // v1: log the override but route through the main ApiClient.
            // Per-role ApiClient construction is a P2 follow-up.
            tracing::debug!(
                requested_model = m,
                bound_model = self.client.model_id(),
                "Prompt hook model override not yet wired; using main client model"
            );
        }

        let prompt = build_prompt(prompt);
        let params = QueryParams {
            prompt,
            max_tokens: Some(1024),
            thinking_level: None,
            fast_mode: false,
            tools: None,
            context_management: None,
            query_source: Some("hook_prompt".into()),
            agent_id: None,
            time_since_last_assistant_ms: None,
            cache: None,
            agentic: false,
        };

        let result = tokio::time::timeout(timeout, self.client.query(&params)).await;

        match result {
            // TS treats timeout as `cancelled` — silent, no UI error.
            Err(_elapsed) => HookEvaluationResult::Cancelled,
            Ok(Err(e)) => HookEvaluationResult::NonBlockingError {
                error: format!("hook prompt API error: {e}"),
            },
            Ok(Ok(query_result)) => parse_hook_response(&query_result.content),
        }
    }

    async fn evaluate_agent(
        &self,
        _prompt: &str,
        _model: Option<&str>,
        _timeout: Duration,
    ) -> HookEvaluationResult {
        // v1 stub: full multi-turn agent evaluation requires a forked
        // `QueryEngine`, `StructuredOutputTool`, transcript-read
        // permission grant, and "must call structured output" stop-hook
        // enforcement. Until that lands, return `Cancelled` — TS's
        // own fallback when `MAX_AGENT_TURNS` is hit or no
        // StructuredOutput call is observed (`execAgentHook.ts:248-267`).
        // `Cancelled` is silent (no UI error), so users with Agent
        // hooks configured see "no effect" rather than spurious
        // failures, which is the conservative degradation.
        tracing::warn!(
            "Agent hook evaluation falling back to Cancelled — full implementation is pending. \
             See crate-coco-hooks.md P3 follow-up. TS reference: utils/hooks/execAgentHook.ts."
        );
        HookEvaluationResult::Cancelled
    }
}

/// Build the message prompt for an LLM hook evaluation.
///
/// Two-message shape: `System` carries the JSON-output instruction
/// (verbatim from TS `execPromptHook.ts`); `User` carries the user's
/// hook prompt with `$ARGUMENTS` already substituted upstream by
/// `run_hook_via_handle_or_fallback`.
fn build_prompt(user_prompt: &str) -> Vec<LanguageModelMessage> {
    vec![
        LanguageModelMessage::System {
            content: vec![UserContentPart::text(HOOK_PROMPT_SYSTEM)],
            provider_options: None,
        },
        LanguageModelMessage::User {
            content: vec![UserContentPart::text(user_prompt)],
            provider_options: None,
        },
    ]
}

/// Parse the assistant's text response as `{ok, reason}` JSON.
///
/// Failure modes:
/// - No text part in the response → NonBlockingError
/// - Text is not valid JSON or doesn't match `HookResponse` → NonBlockingError
/// - `ok: false` → Blocking with the supplied reason
/// - `ok: true` → Ok
fn parse_hook_response(content: &[AssistantContentPart]) -> HookEvaluationResult {
    // Multi-text-part assistant messages are now possible (streaming
    // path preserves per-part `provider_metadata`). The naive `join("")`
    // still works for hook LLM responses because hooks emit a single
    // JSON object as text; multi-text would corrupt the parse but the
    // existing test
    // (`test_parse_hook_response_concatenates_multiple_text_parts`)
    // verifies that the parser tolerates the multi-text shape and
    // returns a parse-failure outcome rather than crashing.
    let text = content
        .iter()
        .filter_map(|part| match part {
            AssistantContentPart::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string();

    if text.is_empty() {
        return HookEvaluationResult::NonBlockingError {
            error: "hook prompt returned empty assistant text".into(),
        };
    }

    let parsed = match serde_json::from_str::<HookResponse>(&text) {
        Ok(p) => p,
        Err(e) => {
            return HookEvaluationResult::NonBlockingError {
                error: format!("schema validation failed: {e} — raw response: {text}"),
            };
        }
    };

    if parsed.ok {
        HookEvaluationResult::Ok
    } else {
        HookEvaluationResult::Blocking {
            reason: parsed
                .reason
                .unwrap_or_else(|| "Prompt hook condition not met".into()),
        }
    }
}

#[cfg(test)]
#[path = "hook_llm.test.rs"]
mod tests;
