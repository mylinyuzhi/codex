use coco_types::ProviderApi;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;
use std::collections::HashMap;

/// Convert a ThinkingLevel into provider-specific options.
///
/// Step 1: effort + budget_tokens → per-provider typed conversion
/// Step 2: level.options → merge directly into ProviderOptions (passthrough)
pub fn convert_thinking_level(
    level: &ThinkingLevel,
    provider: ProviderApi,
) -> HashMap<String, serde_json::Value> {
    let mut result = HashMap::new();

    if level.effort == ReasoningEffort::None {
        return result;
    }

    match provider {
        ProviderApi::Anthropic => {
            // Anthropic: thinking { type, budget_tokens }
            let mut thinking = serde_json::Map::new();
            thinking.insert("type".into(), serde_json::Value::String("enabled".into()));
            if let Some(budget) = level.budget_tokens {
                thinking.insert(
                    "budget_tokens".into(),
                    serde_json::Value::Number(budget.into()),
                );
            }
            result.insert("thinking".into(), serde_json::Value::Object(thinking));
        }
        ProviderApi::Openai => {
            // OpenAI: reasoning_effort
            let effort_str = match level.effort {
                ReasoningEffort::None | ReasoningEffort::Minimal => "low",
                ReasoningEffort::Low => "low",
                ReasoningEffort::Medium => "medium",
                ReasoningEffort::High | ReasoningEffort::XHigh => "high",
            };
            result.insert(
                "reasoning_effort".into(),
                serde_json::Value::String(effort_str.into()),
            );
        }
        ProviderApi::Gemini => {
            // Google: thinkingConfig
            let mut config = serde_json::Map::new();
            config.insert(
                "thinkingLevel".into(),
                serde_json::Value::String(level.effort.to_string()),
            );
            if let Some(budget) = level.budget_tokens {
                config.insert(
                    "thinkingBudget".into(),
                    serde_json::Value::Number(budget.into()),
                );
            }
            result.insert("thinkingConfig".into(), serde_json::Value::Object(config));
        }
        ProviderApi::Volcengine | ProviderApi::Zai => {
            // Budget-based: similar to Anthropic
            if let Some(budget) = level.budget_tokens {
                result.insert(
                    "thinking_budget".into(),
                    serde_json::Value::Number(budget.into()),
                );
            }
        }
        ProviderApi::OpenaiCompat => {
            // Pass through as generic reasoning params
            result.insert(
                "reasoning_effort".into(),
                serde_json::Value::String(level.effort.to_string()),
            );
        }
    }

    // Step 2: Merge options passthrough
    for (key, value) in &level.options {
        result.insert(key.clone(), value.clone());
    }

    result
}

#[cfg(test)]
#[path = "thinking_convert.test.rs"]
mod tests;
