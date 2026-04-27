//! `ThinkingLevel` → flat camelCase `extra_body` keys.
//!
//! Output is a `BTreeMap<String, JSONValue>` — provider-aware mapping
//! that produces the same shape a user would write directly into
//! `ModelInfo.extra_body`. Layer 2 (`build_call_options`) merges this
//! into the per-call `extra_body` and wraps under the SDK namespace
//! key.
//!
//! All keys are camelCase so they match each provider's typed-options
//! struct (`#[serde(rename_all = "camelCase")]` on
//! `AnthropicProviderOptions`, `OpenAIResponsesProviderOptions`,
//! `GoogleLanguageModelOptions`, etc.).
//!
//! **Routing key is `ProviderApi`, not the runtime instance name.**
//! A `ProviderConfig.name = "azure-east"` backed by
//! `api: ProviderApi::Anthropic` still emits Anthropic's
//! `thinking { type, budgetTokens }` shape — the family is what
//! determines wire body, not the user-facing instance label.

use coco_types::ProviderApi;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;
use std::collections::BTreeMap;

/// Convert a `ThinkingLevel` into provider-neutral flat camelCase
/// keys. Routing key is the wire-protocol family.
pub fn to_extra_body(
    level: &ThinkingLevel,
    api: ProviderApi,
) -> BTreeMap<String, serde_json::Value> {
    let mut out = BTreeMap::new();

    if level.effort == ReasoningEffort::None {
        return out;
    }

    match api {
        ProviderApi::Anthropic => {
            // { "thinking": { "type": "enabled", "budgetTokens": <n> } }
            let mut thinking = serde_json::Map::new();
            thinking.insert("type".into(), serde_json::Value::String("enabled".into()));
            if let Some(budget) = level.budget_tokens {
                thinking.insert(
                    "budgetTokens".into(),
                    serde_json::Value::Number(budget.into()),
                );
            }
            out.insert("thinking".into(), serde_json::Value::Object(thinking));
        }
        ProviderApi::Openai => {
            // OpenAI Responses: { "reasoningSummary": "auto" }. Effort
            // is sent via the `reasoning` typed field on
            // `LanguageModelV4CallOptions`, not via extra_body.
            out.insert(
                "reasoningSummary".into(),
                serde_json::Value::String("auto".into()),
            );
        }
        ProviderApi::Gemini => {
            // { "thinkingConfig": { "includeThoughts": true, "thinkingBudget": <n> } }
            let mut config = serde_json::Map::new();
            config.insert("includeThoughts".into(), serde_json::Value::Bool(true));
            if let Some(budget) = level.budget_tokens {
                config.insert(
                    "thinkingBudget".into(),
                    serde_json::Value::Number(budget.into()),
                );
            }
            out.insert("thinkingConfig".into(), serde_json::Value::Object(config));
        }
        ProviderApi::Volcengine | ProviderApi::Zai | ProviderApi::OpenaiCompat => {
            // xAI / DeepSeek / Volcengine / Z.AI / generic compat:
            // { "reasoningEffort": "high" }
            out.insert(
                "reasoningEffort".into(),
                serde_json::Value::String(level.effort.to_string()),
            );
        }
    }

    // Pass-through `level.options` — provider-specific extensions land
    // verbatim. Caller is responsible for camelCase keys (the typed
    // fields above are camelCase by construction).
    for (key, value) in &level.options {
        out.insert(key.clone(), value.clone());
    }

    out
}

#[cfg(test)]
#[path = "thinking_convert.test.rs"]
mod tests;
