//! Prompt cache strategy for Anthropic API.
//!
//! Injects `cache_control` markers into the prompt to enable Anthropic's
//! prompt caching, reducing API costs by up to 90% for cached content.
//!
//! Two injection points:
//! 1. **System prompt blocks** — via `ProviderOptions` on `System` messages
//! 2. **Message breakpoints** — via `ProviderOptions` on the last message
//!
//! The vercel-ai/anthropic conversion layer reads these options and applies
//! `cache_control` to the corresponding API request blocks.

use std::collections::HashMap;

use crate::LanguageModelMessage;
use crate::ProviderOptions;
use crate::request_options_merge::build_options;
use cocode_protocol::CacheScope;
use cocode_protocol::PromptCacheConfig;
use cocode_protocol::ProviderApi;

/// Apply message-level cache breakpoints to the prompt.
///
/// Sets `provider_options` with `cache_control` on the breakpoint message.
/// The Anthropic conversion layer picks this up via the message-level
/// fallback path and applies it to the last content part.
///
/// Equivalent of Claude Code's `applyCacheBreakpointsToMessages` (z9z).
pub fn apply_message_breakpoints(
    prompt: &mut [LanguageModelMessage],
    config: &PromptCacheConfig,
    api: ProviderApi,
    model_slug: &str,
) {
    if api != ProviderApi::Anthropic {
        return;
    }

    if !config.enabled || is_caching_disabled(model_slug) {
        return;
    }

    if prompt.is_empty() {
        return;
    }

    // Breakpoint index: last message, or second-to-last when skipping cache writes
    let breakpoint_idx = if config.skip_cache_write {
        prompt.len().saturating_sub(2)
    } else {
        prompt.len() - 1
    };

    // Skip system messages (they get cache_control via build_for_cache blocks)
    if matches!(
        prompt.get(breakpoint_idx),
        Some(LanguageModelMessage::System { .. })
    ) {
        return;
    }

    let cache_control = ephemeral_cache_control();

    if let Some(msg) = prompt.get_mut(breakpoint_idx) {
        set_message_cache_control(msg, cache_control);
    }
}

/// Build `ProviderOptions` for a system prompt block with the given cache scope.
///
/// Returns `None` if no cache_control should be applied (scope is None).
///
/// Equivalent of Claude Code's `createCacheControl` (Ml) applied to system blocks.
pub fn build_cache_provider_options(scope: Option<CacheScope>) -> Option<ProviderOptions> {
    // Blocks without scope get no cache_control
    scope.as_ref()?;

    let mut cc = serde_json::json!({"type": "ephemeral"});
    // Only include scope for Global (matching Claude Code: scope only for "global")
    if scope == Some(CacheScope::Global) {
        cc["scope"] = serde_json::json!("global");
    }

    let mut opts = HashMap::new();
    opts.insert("cacheControl".to_string(), cc);
    Some(build_options("anthropic", opts))
}

/// Check if prompt caching is disabled via environment variables.
///
/// Checks both global and model-specific disable flags.
/// Equivalent of Claude Code's `isPromptCachingEnabled` (IGq).
fn is_caching_disabled(model_slug: &str) -> bool {
    if is_env_truthy("DISABLE_PROMPT_CACHING") {
        return true;
    }
    let slug_lower = model_slug.to_lowercase();
    if slug_lower.contains("haiku") && is_env_truthy("DISABLE_PROMPT_CACHING_HAIKU") {
        return true;
    }
    if slug_lower.contains("sonnet") && is_env_truthy("DISABLE_PROMPT_CACHING_SONNET") {
        return true;
    }
    if slug_lower.contains("opus") && is_env_truthy("DISABLE_PROMPT_CACHING_OPUS") {
        return true;
    }
    false
}

fn is_env_truthy(var: &str) -> bool {
    std::env::var(var)
        .ok()
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

/// Build the ephemeral cache_control JSON value.
fn ephemeral_cache_control() -> serde_json::Value {
    serde_json::json!({"type": "ephemeral"})
}

/// Set cache_control on a message via provider_options.
///
/// The Anthropic conversion layer reads `provider_options.anthropic.cacheControl`
/// and applies it as a fallback to the last content part of the message.
fn set_message_cache_control(msg: &mut LanguageModelMessage, cache_control: serde_json::Value) {
    let opts = build_anthropic_cache_options(cache_control);
    match msg {
        LanguageModelMessage::User {
            provider_options, ..
        }
        | LanguageModelMessage::Assistant {
            provider_options, ..
        }
        | LanguageModelMessage::Tool {
            provider_options, ..
        } => {
            *provider_options = Some(opts);
        }
        LanguageModelMessage::System { .. } => {}
    }
}

/// Build ProviderOptions with Anthropic cacheControl.
fn build_anthropic_cache_options(cache_control: serde_json::Value) -> ProviderOptions {
    let mut opts = HashMap::new();
    opts.insert("cacheControl".to_string(), cache_control);
    build_options("anthropic", opts)
}

#[cfg(test)]
#[path = "prompt_cache.test.rs"]
mod tests;
