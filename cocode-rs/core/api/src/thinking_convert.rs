//! ThinkingLevel to vercel-ai ProviderOptions conversion.
//!
//! Converts the unified `ThinkingLevel` from `cocode_protocol` to
//! provider-specific options using vercel-ai's HashMap-based `ProviderOptions`.

use crate::ProviderOptions;
use crate::ReasoningLevel;
use crate::request_options_merge::build_options;
use cocode_protocol::ModelInfo;
use cocode_protocol::ProviderApi;
use cocode_protocol::ThinkingLevel;
use cocode_protocol::model::ReasoningEffort;
use cocode_protocol::model::ReasoningSummary;
use serde_json::json;
use std::collections::HashMap;
use vercel_ai_anthropic::messages::anthropic_messages_options::ThinkingConfig as AnthropicThinking;
use vercel_ai_google::ThinkingConfig as GoogleThinkingConfig;
use vercel_ai_google::ThinkingLevel as GoogleThinkingLevel;

/// Convert ThinkingLevel and ModelInfo to provider-specific options.
///
/// Returns `None` if thinking is disabled or the provider doesn't support it.
pub fn to_provider_options(
    level: &ThinkingLevel,
    model_info: &ModelInfo,
    provider: ProviderApi,
) -> Option<ProviderOptions> {
    if !level.is_enabled() {
        return None;
    }

    match provider {
        ProviderApi::Anthropic => to_anthropic_options(level),
        ProviderApi::Openai | ProviderApi::OpenaiCompat => to_openai_options(level, model_info),
        ProviderApi::Gemini => to_gemini_options(level, model_info),
        ProviderApi::Volcengine => to_volcengine_options(level),
        ProviderApi::Zai => to_zai_options(level),
    }
}

/// Anthropic: Adaptive (no budget) or Enabled (with budget) for extended thinking.
fn to_anthropic_options(level: &ThinkingLevel) -> Option<ProviderOptions> {
    let thinking = match level.budget_tokens {
        Some(budget) => AnthropicThinking::Enabled {
            budget_tokens: Some(budget as u64),
        },
        None => AnthropicThinking::Adaptive,
    };
    let Ok(val) = serde_json::to_value(thinking) else {
        return None;
    };
    let mut opts = HashMap::new();
    opts.insert("thinking".to_string(), val);
    Some(build_options("anthropic", opts))
}

/// OpenAI: reasoning effort + summary.
fn to_openai_options(level: &ThinkingLevel, model_info: &ModelInfo) -> Option<ProviderOptions> {
    let effort = map_to_openai_effort(level.effort)?;
    let mut opts = HashMap::new();
    opts.insert("reasoningEffort".to_string(), json!(effort));

    // Apply reasoning summary from ModelInfo
    if let Some(summary) = &model_info.reasoning_summary
        && let Some(oai_summary) = map_to_openai_summary(*summary)
    {
        opts.insert("reasoningSummary".to_string(), json!(oai_summary));
    }

    Some(build_options("openai", opts))
}

/// Gemini: nested thinkingConfig with camelCase keys.
fn to_gemini_options(level: &ThinkingLevel, model_info: &ModelInfo) -> Option<ProviderOptions> {
    let gem_level = map_to_google_thinking_level(level.effort)?;
    let include = model_info.include_thoughts.unwrap_or(true);
    let config = GoogleThinkingConfig {
        thinking_level: Some(gem_level),
        include_thoughts: Some(include),
        thinking_budget: None,
    };
    let Ok(val) = serde_json::to_value(config) else {
        return None;
    };
    let mut opts = HashMap::new();
    opts.insert("thinkingConfig".to_string(), val);
    Some(build_options("google", opts))
}

/// Volcengine: budgetTokens + reasoning effort.
fn to_volcengine_options(level: &ThinkingLevel) -> Option<ProviderOptions> {
    let mut opts = HashMap::new();
    let mut has_thinking = false;

    if let Some(budget) = level.budget_tokens {
        opts.insert(
            "thinking".to_string(),
            json!({"type": "enabled", "budgetTokens": budget}),
        );
        has_thinking = true;
    }

    if let Some(effort) = map_to_volcengine_effort(level.effort) {
        opts.insert("reasoningEffort".to_string(), json!(effort));
        has_thinking = true;
    }

    if has_thinking {
        Some(build_options("volcengine", opts))
    } else {
        None
    }
}

/// Z.AI: budgetTokens for extended thinking.
fn to_zai_options(level: &ThinkingLevel) -> Option<ProviderOptions> {
    let budget = level.budget_tokens?;
    let mut opts = HashMap::new();
    opts.insert(
        "thinking".to_string(),
        json!({"type": "enabled", "budgetTokens": budget}),
    );
    Some(build_options("zai", opts))
}

/// Map protocol `ReasoningEffort` to vercel-ai `ReasoningLevel`.
///
/// Returns `None` for `ReasoningEffort::None` (no reasoning requested).
pub fn effort_to_reasoning_level(effort: ReasoningEffort) -> Option<ReasoningLevel> {
    match effort {
        ReasoningEffort::None => None,
        ReasoningEffort::Minimal => Some(ReasoningLevel::Minimal),
        ReasoningEffort::Low => Some(ReasoningLevel::Low),
        ReasoningEffort::Medium => Some(ReasoningLevel::Medium),
        ReasoningEffort::High => Some(ReasoningLevel::High),
        ReasoningEffort::XHigh => Some(ReasoningLevel::Xhigh),
    }
}

// =============================================================================
// Effort Level Mappings
// =============================================================================

fn map_to_openai_summary(summary: ReasoningSummary) -> Option<&'static str> {
    match summary {
        ReasoningSummary::None => None,
        ReasoningSummary::Auto => Some("auto"),
        ReasoningSummary::Concise => Some("concise"),
        ReasoningSummary::Detailed => Some("detailed"),
    }
}

fn map_to_openai_effort(effort: ReasoningEffort) -> Option<&'static str> {
    match effort {
        ReasoningEffort::None => None,
        ReasoningEffort::Minimal | ReasoningEffort::Low => Some("low"),
        ReasoningEffort::Medium => Some("medium"),
        ReasoningEffort::High | ReasoningEffort::XHigh => Some("high"),
    }
}

fn map_to_google_thinking_level(effort: ReasoningEffort) -> Option<GoogleThinkingLevel> {
    match effort {
        ReasoningEffort::None => None,
        ReasoningEffort::Minimal | ReasoningEffort::Low => Some(GoogleThinkingLevel::Low),
        ReasoningEffort::Medium => Some(GoogleThinkingLevel::Medium),
        ReasoningEffort::High | ReasoningEffort::XHigh => Some(GoogleThinkingLevel::High),
    }
}

fn map_to_volcengine_effort(effort: ReasoningEffort) -> Option<&'static str> {
    match effort {
        ReasoningEffort::None => None,
        ReasoningEffort::Minimal => Some("minimal"),
        ReasoningEffort::Low => Some("low"),
        ReasoningEffort::Medium => Some("medium"),
        ReasoningEffort::High | ReasoningEffort::XHigh => Some("high"),
    }
}

#[cfg(test)]
#[path = "thinking_convert.test.rs"]
mod tests;
