//! Adapter-owned parser for the per-provider-instance behavior knobs
//! carried in `ProviderConfig.provider_options`.
//!
//! Schema is owned by **this crate**, not by `coco-config`. The
//! infrastructure layer transports an opaque `BTreeMap<String, Value>`;
//! we deserialize it here into a typed struct with `deny_unknown_fields`
//! so a typo (`disable_interleaved_thinkin`) fails at startup rather
//! than silently shipping the default.
//!
//! `parse_provider_options` is **infallible** for the empty / partial
//! cases (every field has a default). It returns `Err` only when a key
//! is present with the wrong type or shape — that's a config error,
//! not a default to swallow.
//!
//! Settings example (`~/.coco/settings.json` or `~/.coco/providers.json`):
//!
//! ```json
//! {
//!   "providers": {
//!     "anthropic": {
//!       "api": "anthropic",
//!       "base_url": "https://api.anthropic.com/v1",
//!       "env_key": "ANTHROPIC_API_KEY",
//!       "provider_options": {
//!         "experimental_betas": false,
//!         "disable_interleaved_thinking": true
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! Unset fields fall through to the typed defaults below
//! (TS-`betas.ts`-mirroring values).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Resolved Anthropic per-provider behavior knobs. All fields concrete
/// — no `Option` — because callers need a fully-determined view by
/// the time `AnthropicProviderSettings` is constructed.
///
/// Defaults match TS `betas.ts` semantics:
/// - `experimental_betas` defaults `true` (so first-party-only betas
///   like `redact-thinking-2026-02-12`, `prompt-caching-scope-2026-01-05`
///   ship by default; setting `false` opts out).
/// - The other three default `false` (TS env vars `DISABLE_*` /
///   `getInitialSettings().showThinkingSummaries` /
///   `getIsNonInteractiveSession()` are off by default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnthropicProviderOptionsConfig {
    /// Mirrors TS `!DISABLE_EXPERIMENTAL_BETAS` (`betas.ts:215`).
    /// Drives first-party-only beta inclusion (RedactThinking,
    /// PromptCachingScope, ContextManagement). Default `true`.
    pub experimental_betas_enabled: bool,
    /// Mirrors TS `process.env.DISABLE_INTERLEAVED_THINKING`
    /// (`betas.ts:258-262`). Suppresses `interleaved-thinking-2025-05-14`
    /// even on capable models. Default `false`.
    pub disable_interleaved_thinking: bool,
    /// Mirrors TS `getInitialSettings().showThinkingSummaries`
    /// (`betas.ts:268-275`). Suppresses `redact-thinking-2026-02-12`
    /// when `true` (the UI renders raw thinking, redaction is
    /// counter-productive). Default `false`.
    pub show_thinking_summaries: bool,
    /// Mirrors TS `getIsNonInteractiveSession()` (`betas.ts:268-275`).
    /// Suppresses `redact-thinking-2026-02-12` for non-interactive runs
    /// (no human to consume thinking redaction). Default `false`.
    pub non_interactive: bool,
}

impl Default for AnthropicProviderOptionsConfig {
    fn default() -> Self {
        Self {
            experimental_betas_enabled: true,
            disable_interleaved_thinking: false,
            show_thinking_summaries: false,
            non_interactive: false,
        }
    }
}

/// Wire shape — what the JSON in `provider_options` looks like. Every
/// field is Optional so a partial map is valid (missing fields → the
/// default in `AnthropicProviderOptionsConfig`).
///
/// `deny_unknown_fields` makes typos like `disable_interleaved_thinkin`
/// surface at startup as a deserialization error, not at the next
/// `interleaved-thinking-2025-05-14` request.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct AnthropicProviderOptionsRaw {
    experimental_betas: Option<bool>,
    disable_interleaved_thinking: Option<bool>,
    show_thinking_summaries: Option<bool>,
    non_interactive: Option<bool>,
}

/// Errors produced by [`parse_provider_options`]. Typed so callers
/// (services/inference) can attach structured context (provider name,
/// settings source path) when surfacing to the user.
#[derive(Debug, thiserror::Error)]
pub enum ProviderOptionsError {
    /// JSON shape didn't deserialize into the typed schema. Carries the
    /// underlying `serde_json::Error` for the field path / line / column.
    #[error("invalid anthropic provider_options: {0}")]
    Invalid(#[from] serde_json::Error),
}

/// Parse the opaque `BTreeMap<String, Value>` from `ProviderConfig.provider_options`
/// into a typed config. Empty map → all defaults.
///
/// The parser routes through `serde_json::Value` (re-serialize the map
/// then deserialize into the typed struct) rather than walking keys
/// manually. That keeps `deny_unknown_fields` enforcement automatic and
/// matches how `extract_anthropic_options` already does typed parsing
/// of per-call `ProviderOptions`.
pub fn parse_provider_options(
    options: &BTreeMap<String, Value>,
) -> Result<AnthropicProviderOptionsConfig, ProviderOptionsError> {
    if options.is_empty() {
        return Ok(AnthropicProviderOptionsConfig::default());
    }
    // Round-trip through `Value::Object` so serde sees the same shape
    // it would from the wire. Avoids hand-writing a `from_iter` that
    // would diverge from the JSON parser's behavior.
    let value = Value::Object(
        options
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    );
    let raw: AnthropicProviderOptionsRaw = serde_json::from_value(value)?;
    let defaults = AnthropicProviderOptionsConfig::default();
    Ok(AnthropicProviderOptionsConfig {
        experimental_betas_enabled: raw
            .experimental_betas
            .unwrap_or(defaults.experimental_betas_enabled),
        disable_interleaved_thinking: raw
            .disable_interleaved_thinking
            .unwrap_or(defaults.disable_interleaved_thinking),
        show_thinking_summaries: raw
            .show_thinking_summaries
            .unwrap_or(defaults.show_thinking_summaries),
        non_interactive: raw.non_interactive.unwrap_or(defaults.non_interactive),
    })
}

#[cfg(test)]
#[path = "provider_options.test.rs"]
mod tests;
