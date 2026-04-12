use coco_types::ProviderApi;
use serde::Deserialize;
use serde::Serialize;

/// Model aliases for convenient selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAlias {
    Sonnet,
    Opus,
    Haiku,
    Best,
    SonnetLargeCtx,
    OpusLargeCtx,
    OpusPlan,
}

/// Resolve an alias to a concrete model ID for the given provider.
pub fn resolve_alias(alias: ModelAlias, provider: ProviderApi) -> String {
    match (alias, provider) {
        // Anthropic
        (ModelAlias::Sonnet, ProviderApi::Anthropic) => "claude-sonnet-4-6-20250514".into(),
        (ModelAlias::Opus, ProviderApi::Anthropic) => "claude-opus-4-6-20250514".into(),
        (ModelAlias::Haiku, ProviderApi::Anthropic) => "claude-haiku-4-5-20251001".into(),
        (ModelAlias::Best, _) => resolve_alias(ModelAlias::Opus, provider),
        (ModelAlias::SonnetLargeCtx, ProviderApi::Anthropic) => "claude-sonnet-4-6-20250514".into(),
        (ModelAlias::OpusLargeCtx, ProviderApi::Anthropic) => "claude-opus-4-6-20250514".into(),
        (ModelAlias::OpusPlan, _) => resolve_alias(ModelAlias::Opus, provider),

        // OpenAI
        (ModelAlias::Sonnet, ProviderApi::Openai) => "gpt-4o".into(),
        (ModelAlias::Opus, ProviderApi::Openai) => "gpt-4o".into(),
        (ModelAlias::Haiku, ProviderApi::Openai) => "gpt-4o-mini".into(),

        // Google
        (ModelAlias::Sonnet, ProviderApi::Gemini) => "gemini-2.5-pro".into(),
        (ModelAlias::Opus, ProviderApi::Gemini) => "gemini-2.5-pro".into(),
        (ModelAlias::Haiku, ProviderApi::Gemini) => "gemini-2.5-flash".into(),

        // Fallback for other providers
        (_, _) => "claude-sonnet-4-6-20250514".into(),
    }
}

/// Parse user model input (alias or direct model ID).
pub fn parse_user_model(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "sonnet" => resolve_alias(ModelAlias::Sonnet, ProviderApi::Anthropic),
        "opus" => resolve_alias(ModelAlias::Opus, ProviderApi::Anthropic),
        "haiku" => resolve_alias(ModelAlias::Haiku, ProviderApi::Anthropic),
        "best" => resolve_alias(ModelAlias::Best, ProviderApi::Anthropic),
        _ => input.to_string(),
    }
}

#[cfg(test)]
#[path = "aliases.test.rs"]
mod tests;
