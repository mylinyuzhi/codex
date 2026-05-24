//! Unified model specification type.

use crate::ProviderApi;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::fmt;
use std::hash::Hash;
use std::str::FromStr;

/// Resolve a provider string to a ProviderApi enum.
///
/// This maps common provider names to their ProviderApi enum values.
/// Unknown providers default to OpenaiCompat for maximum compatibility.
///
/// # Examples
///
/// ```
/// use cocode_protocol::model::resolve_provider_api;
/// use cocode_protocol::ProviderApi;
///
/// assert_eq!(resolve_provider_api("anthropic"), ProviderApi::Anthropic);
/// assert_eq!(resolve_provider_api("openai"), ProviderApi::Openai);
/// assert_eq!(resolve_provider_api("unknown"), ProviderApi::OpenaiCompat);
/// ```
pub fn resolve_provider_api(provider: &str) -> ProviderApi {
    // Normalize provider name to lowercase for comparison
    match provider.to_lowercase().as_str() {
        "anthropic" => ProviderApi::Anthropic,
        "openai" => ProviderApi::Openai,
        "gemini" | "genai" | "google" => ProviderApi::Gemini,
        "volcengine" | "ark" => ProviderApi::Volcengine,
        "zai" | "zhipu" | "zhipuai" => ProviderApi::Zai,
        "openai_compat" | "openai-compat" => ProviderApi::OpenaiCompat,
        // Default to OpenaiCompat for unknown providers (most compatible)
        _ => ProviderApi::OpenaiCompat,
    }
}

/// Unified model specification: "{provider}/{model}" with resolved provider API.
///
/// Provides a single string format for specifying both provider and model,
/// along with the resolved `ProviderApi` for API dispatch.
///
/// # Examples
///
/// ```
/// use cocode_protocol::model::ModelSpec;
/// use cocode_protocol::ProviderApi;
///
/// let spec: ModelSpec = "anthropic/claude-opus-4".parse().unwrap();
/// assert_eq!(spec.provider, "anthropic");
/// assert_eq!(spec.slug, "claude-opus-4");
/// assert_eq!(spec.api, ProviderApi::Anthropic);
/// assert_eq!(spec.to_string(), "anthropic/claude-opus-4");
/// ```
#[derive(Debug, Clone)]
pub struct ModelSpec {
    /// Provider name (e.g., "anthropic", "openai", "genai").
    pub provider: String,
    /// Resolved provider type for API dispatch.
    pub api: ProviderApi,
    /// Model slug — the config key (e.g., "claude-opus-4", "gpt-5").
    ///
    /// This is the identifier used in configuration and caching.
    /// The actual API model name may differ (e.g., Volcengine endpoint IDs);
    /// use `ProviderModel::api_model_name()` to get the name sent to the API.
    pub slug: String,
    /// Human-readable display name (e.g., "GPT-5"). Defaults to slug.
    /// Not part of identity (excluded from PartialEq/Hash/Serialize).
    pub display_name: String,
}

impl PartialEq for ModelSpec {
    fn eq(&self, other: &Self) -> bool {
        self.provider == other.provider && self.api == other.api && self.slug == other.slug
    }
}

impl Eq for ModelSpec {}

impl Hash for ModelSpec {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.provider.hash(state);
        self.api.hash(state);
        self.slug.hash(state);
    }
}

impl ModelSpec {
    /// Create a new model specification with auto-resolved provider type.
    ///
    /// The provider type is automatically resolved from the provider name.
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        let provider = provider.into();
        let api = resolve_provider_api(&provider);
        let model = model.into();
        let display_name = model.clone();
        Self {
            provider,
            api,
            slug: model,
            display_name,
        }
    }

    /// Create a new model specification with explicit provider type.
    ///
    /// Use this when you know the exact provider type and don't want
    /// to rely on string-based resolution.
    pub fn with_type(
        provider: impl Into<String>,
        api: ProviderApi,
        model: impl Into<String>,
    ) -> Self {
        let model = model.into();
        let display_name = model.clone();
        Self {
            provider: provider.into(),
            api,
            slug: model,
            display_name,
        }
    }

    /// Set a custom display name.
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }

    /// Get the model slug.
    pub fn slug(&self) -> &str {
        &self.slug
    }

    /// Enrich display_name from ModelInfo.
    pub fn enrich_from_model_info(&mut self, info: &super::ModelInfo) {
        self.display_name = info.display_name_or_slug().to_string();
    }

    /// Parse a model string that may or may not include a provider prefix.
    ///
    /// If the string contains `"provider/model"`, both parts are used.
    /// If it contains only `"model"`, the provider defaults to `"anthropic"`.
    pub fn parse_with_default_provider(s: &str) -> Self {
        if let Some((provider, model)) = s.split_once('/') {
            Self::new(provider, model)
        } else {
            Self::new("anthropic", s)
        }
    }
}

impl fmt::Display for ModelSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.provider, self.slug)
    }
}

/// Error returned when parsing a `ModelSpec` from a string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSpecParseError(pub String);

impl fmt::Display for ModelSpecParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ModelSpecParseError {}

impl FromStr for ModelSpec {
    type Err = ModelSpecParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(2, '/').collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(ModelSpecParseError(format!(
                "invalid format: expected 'provider/model', got '{s}'"
            )));
        }
        // new() automatically resolves provider_api from provider string
        Ok(Self::new(parts[0], parts[1]))
    }
}

impl From<(String, ProviderApi, String)> for ModelSpec {
    fn from((provider, api, model): (String, ProviderApi, String)) -> Self {
        let display_name = model.clone();
        Self {
            provider,
            api,
            slug: model,
            display_name,
        }
    }
}

impl Serialize for ModelSpec {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ModelSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse()
            .map_err(|e: ModelSpecParseError| serde::de::Error::custom(e.0))
    }
}

#[cfg(test)]
#[path = "model_spec.test.rs"]
mod tests;
