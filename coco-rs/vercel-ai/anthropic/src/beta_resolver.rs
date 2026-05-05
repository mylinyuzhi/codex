//! Resolved-beta computation: capability gates → wire header set.
//!
//! Single source of truth for "which betas should this request emit".
//! Mirrors TS `getModelDependentBetas` (`betas.ts:106-180`) +
//! `getBetas` baseline (`betas.ts:181-263`) into one function so the
//! gate logic is auditable in one place rather than scattered across
//! `get_args`, `prepare_tools`, and per-feature insert sites.
//!
//! Inputs are all already-resolved at the adapter boundary:
//! - **Model capabilities** (`AnthropicModelCapabilities`) — set by
//!   the provider factory from `ResolvedModel.info.capabilities`.
//! - **Endpoint topology** (`ProviderTopology`) — gates the
//!   first-party-only set.
//! - **Knobs** (`experimental_betas_enabled`, `disable_interleaved_thinking`,
//!   `show_thinking_summaries`, `non_interactive`) — settings
//!   gates parsed from `ProviderConfig.provider_options` via
//!   `parse_provider_options` into `AnthropicProviderOptionsConfig`.
//! - **Per-call signals** (`agentic_query`, `requested_betas`) — from
//!   `AnthropicProviderOptions`.
//!
//! Output is a `BTreeSet<String>` so the wire-side join is
//! deterministic (TS-parity Finding 7).
//!
//! Design §10.4.

use std::collections::BTreeSet;

use crate::anthropic_config::AnthropicConfig;
use crate::anthropic_config::ProviderTopology;
use crate::beta_capabilities::CLAUDE_CODE_BASELINE;
use crate::beta_capabilities::map_capability;
use crate::messages::anthropic_messages_options::AdapterBetaCapability;

/// Set of beta header strings to send on this request, plus auxiliary
/// flags consumed by `get_args` (e.g. whether the central
/// context-management beta predicate fired — needed at two emission
/// sites: `body["context_management"]` and the memory tool entry in
/// `prepare_tools`).
#[derive(Debug, Clone, Default)]
pub struct ResolvedBetas {
    /// Wire header values, sorted (BTreeSet) — deterministic join.
    pub headers: BTreeSet<String>,
    /// `true` when the (capabilities + knobs + topology) predicate
    /// admits `context-management-2025-06-27`. Memory tool
    /// (`prepare_tools.rs`) gates on the SAME predicate so the beta
    /// can never be emitted by one site and silently dropped by the
    /// other (R3-F2).
    pub context_management_eligible: bool,
}

/// Compute the resolved set. Pure: no I/O, no time, no random.
///
/// `requested_betas` is the user-supplied per-call top-up; each entry
/// passes through `map_capability` for the wire string. Unknown variants
/// (none today) are silently skipped — the typed enum is closed.
pub fn resolve(
    config: &AnthropicConfig,
    agentic_query: bool,
    requested_betas: &[AdapterBetaCapability],
) -> ResolvedBetas {
    let mut headers = BTreeSet::new();

    // Baseline. Helper calls (compaction, title generation) skip this so
    // they don't bill against the agentic-loop baseline.
    if agentic_query {
        headers.insert(CLAUDE_CODE_BASELINE.into());
    }

    // Per-call requested betas — translated through the typed enum.
    for cap in requested_betas {
        if let Some(s) = map_capability(*cap) {
            headers.insert(s.into());
        }
    }

    // Capability-driven beta inclusion. Each gate is a dedicated check
    // so adding a new capability (e.g. a future "json-mode-v2") is a
    // single arm + a single `headers.insert` line.
    let caps = &config.capabilities;
    // Context-1m is OK on every topology and ignores the
    // experimental gate (TS `betas.ts:130-148`).
    if caps.context_1m
        && let Some(h) = map_capability(AdapterBetaCapability::Context1m)
    {
        headers.insert(h.into());
    }
    if should_emit_interleaved_thinking(config)
        && let Some(h) = map_capability(AdapterBetaCapability::InterleavedThinking)
    {
        headers.insert(h.into());
    }
    let context_management_eligible = should_emit_context_management(config);
    if context_management_eligible
        && let Some(h) = map_capability(AdapterBetaCapability::ContextManagement)
    {
        headers.insert(h.into());
    }
    if should_emit_redact_thinking(config)
        && let Some(h) = map_capability(AdapterBetaCapability::RedactThinking)
    {
        headers.insert(h.into());
    }
    // Token-efficient tools is per-model, no extra gate. Provider
    // factory sets the capability bool from registry.
    if caps.token_efficient_tools
        && let Some(h) = map_capability(AdapterBetaCapability::TokenEfficientTools)
    {
        headers.insert(h.into());
    }
    // Prompt-caching-scope: first-party-only AND experimental-gate-on
    // (TS `betas.ts:215-232`).
    if matches!(config.provider_topology, ProviderTopology::FirstParty)
        && config.experimental_betas_enabled
        && let Some(h) = map_capability(AdapterBetaCapability::PromptCachingScope)
    {
        headers.insert(h.into());
    }

    ResolvedBetas {
        headers,
        context_management_eligible,
    }
}

/// Shared predicate so `body["context_management"]` insertion in
/// `get_args` and the memory tool branch in `prepare_tools` agree
/// byte-for-byte. Single source of truth — Finding R3-F2.
///
/// Public so `prepare_tools` can call it without a circular dep.
pub fn should_emit_context_management(config: &AnthropicConfig) -> bool {
    config.capabilities.context_management
        && matches!(config.provider_topology, ProviderTopology::FirstParty)
        && config.experimental_betas_enabled
}

fn should_emit_interleaved_thinking(config: &AnthropicConfig) -> bool {
    config.capabilities.interleaved_thinking && !config.disable_interleaved_thinking
}

/// `redact-thinking-2026-02-12` is first-party-only AND piggybacks on
/// the same `interleaved_thinking` capability (TS `betas.ts:268-275`).
/// Suppressed when `show_thinking_summaries` is on (UI is rendering
/// raw thinking) or `non_interactive` is true (no UI to render
/// redactions).
fn should_emit_redact_thinking(config: &AnthropicConfig) -> bool {
    config.capabilities.interleaved_thinking
        && matches!(config.provider_topology, ProviderTopology::FirstParty)
        && config.experimental_betas_enabled
        && !config.show_thinking_summaries
        && !config.non_interactive
}

#[cfg(test)]
#[path = "beta_resolver.test.rs"]
mod tests;
