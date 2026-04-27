//! `build_call_options` — Layer 2 entry that constructs a fresh
//! `LanguageModelV4CallOptions` per turn.
//!
//! This is the **single ProviderOptions write site** in the entire
//! Coco codebase. Every other place reads `ModelInfo.extra_body`. The
//! function:
//!
//! 1. Wires Lane A typed sampling fields (`temperature`, `top_p`,
//!    `top_k`, `max_output_tokens`) — `None` means "let provider
//!    default", carried through to the wire body unchanged.
//! 2. Maps reasoning channel (Lane A2) — `ThinkingLevel.effort` flows
//!    through `call.reasoning`; budget / interleaved / summary go
//!    through `extra_body` via [`thinking_convert::to_extra_body`].
//!    `Some(level)` with `effort == None` disables thinking entirely
//!    rather than falling through to the model default.
//! 3. Shallow-merges `info.extra_body` ⊕ `per_call.extra_body` ⊕
//!    `thinking_extra` (per-call wins; thinking last) into a single
//!    flat `BTreeMap`.
//! 4. Wraps the merged map under
//!    `provider_options[<canonical_namespace>]`. The namespace key is
//!    derived from the [`ProviderApi`] for builtin SDKs (where
//!    `model.provider()` returns a hardcoded name) and from
//!    `ProviderConfig.name` for OpenAI-compat / Volcengine / Z.AI
//!    instances (where the SDK pass-through honors the configured
//!    `provider_id`).
//!
//! No `as u64` casts: `PositiveTokens` / `PositiveCount` provide
//! `From → u64` infallibly (see `coco_config::positive`).

use crate::thinking_convert;
use coco_types::ProviderApi;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;
use std::collections::BTreeMap;
use std::collections::HashMap;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4Prompt;
use vercel_ai_provider::LanguageModelV4Tool;
use vercel_ai_provider::ProviderOptions;
use vercel_ai_provider::ReasoningLevel;

/// Per-call deltas applied on top of the resolved `ModelInfo`. Each
/// field overrides the corresponding model-level value when `Some`.
/// `extra_body` keys merge with model-level keys; per-call wins.
#[derive(Debug, Clone, Default)]
pub struct PerCallOverrides {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<coco_config::PositiveCount>,
    pub max_output_tokens: Option<coco_config::PositiveTokens>,
    /// Per-turn thinking override.
    /// - `None` — fall through to `info.default_thinking()`.
    /// - `Some(level)` with `effort == ReasoningEffort::None` —
    ///   explicitly disable thinking for this turn.
    /// - `Some(level)` with effort != None — use this level.
    pub thinking_level: Option<ThinkingLevel>,
    pub extra_body: BTreeMap<String, serde_json::Value>,
}

/// Build a fresh `LanguageModelV4CallOptions` for a turn.
pub fn build_call_options(
    info: &coco_config::ModelInfo,
    api: ProviderApi,
    provider_name: &str,
    per_call: &PerCallOverrides,
    prompt: LanguageModelV4Prompt,
    tools: Option<Vec<LanguageModelV4Tool>>,
) -> LanguageModelV4CallOptions {
    let mut call = LanguageModelV4CallOptions {
        prompt,
        ..Default::default()
    };
    call.tools = tools;

    // Lane A: typed sampling. `None` semantically means "let provider
    // default" — every typed body builder writes the field only on
    // `Some`, so omitting it preserves provider-tuned defaults.
    call.temperature = per_call.temperature.or(info.temperature);
    call.top_p = per_call.top_p.or(info.top_p);
    call.top_k = per_call.top_k.or(info.top_k).map(u64::from);
    call.max_output_tokens = per_call
        .max_output_tokens
        .map(u64::from)
        .or_else(|| Some(u64::from(info.max_output_tokens)));

    // Lane A2: typed reasoning channel.
    //
    // Resolution semantics:
    //   per_call.thinking_level == Some(t) where t.effort == None  → disable thinking (Some(t))
    //   per_call.thinking_level == Some(t) where t.effort != None  → use t
    //   per_call.thinking_level == None                            → info.default_thinking()
    //
    // Critically, an explicit per-call effort = None must NOT silently
    // fall through to the model default — that would let a turn
    // disable thinking only for it to come back via the model.
    let thinking: Option<&ThinkingLevel> = match per_call.thinking_level.as_ref() {
        Some(t) => Some(t),
        None => info.default_thinking(),
    };
    if let Some(t) = thinking
        && t.effort != ReasoningEffort::None
    {
        call.reasoning = Some(reasoning_effort_to_level(t.effort));
    }

    // Lane B: shallow-merge extra_body and wrap under the canonical
    // namespace. `BTreeMap` so key order in tests / snapshots is deterministic.
    let mut extra: BTreeMap<String, serde_json::Value> = info.extra_body.clone();
    for (k, v) in &per_call.extra_body {
        extra.insert(k.clone(), v.clone());
    }
    if let Some(t) = thinking
        && t.effort != ReasoningEffort::None
    {
        for (k, v) in thinking_convert::to_extra_body(t, api) {
            extra.insert(k, v);
        }
    }

    if !extra.is_empty() {
        let mut po = ProviderOptions::default();
        let inner: HashMap<String, serde_json::Value> = extra.into_iter().collect();
        po.set(canonical_namespace_key(api, provider_name), inner);
        call.provider_options = Some(po);
    }

    call
}

/// Resolve the namespace key the language-model implementation will
/// read from `call.provider_options`. Closes the round-trip §6.3
/// promises for builtin SDKs whose `model.provider()` is hardcoded.
///
/// - `Anthropic` / `Openai` / `Gemini` — SDK hardcodes the family
///   name (`"anthropic"`, `"openai"`, `"google"`) regardless of
///   `ProviderSettings.provider_id`. The wrap key MUST match.
/// - `OpenaiCompat` / `Volcengine` / `Zai` — SDK passes the
///   `provider_id` through; the runtime instance name (e.g.
///   `"azure-east"`, `"xai"`, `"volcengine"`) is what
///   `model.provider()` returns.
pub fn canonical_namespace_key(api: ProviderApi, provider_name: &str) -> &str {
    match api {
        ProviderApi::Anthropic => "anthropic",
        ProviderApi::Openai => "openai",
        ProviderApi::Gemini => "google",
        ProviderApi::OpenaiCompat | ProviderApi::Volcengine | ProviderApi::Zai => provider_name,
    }
}

/// Map a coco-types `ReasoningEffort` to the vercel-ai
/// `ReasoningLevel` that flows through `LanguageModelV4CallOptions.reasoning`.
fn reasoning_effort_to_level(effort: ReasoningEffort) -> ReasoningLevel {
    match effort {
        ReasoningEffort::None => ReasoningLevel::None,
        ReasoningEffort::Minimal => ReasoningLevel::Minimal,
        ReasoningEffort::Low => ReasoningLevel::Low,
        ReasoningEffort::Medium => ReasoningLevel::Medium,
        ReasoningEffort::High => ReasoningLevel::High,
        ReasoningEffort::XHigh => ReasoningLevel::Xhigh,
    }
}

#[cfg(test)]
#[path = "build_call_options.test.rs"]
mod tests;
