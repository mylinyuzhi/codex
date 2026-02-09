//! Unified model specification type.

use crate::ProviderType;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use std::fmt;
use std::str::FromStr;

/// Resolve a provider string to a ProviderType enum.
///
/// This maps common provider names to their ProviderType enum values.
/// Unknown providers default to OpenaiCompat for maximum compatibility.
///
/// # Examples
///
/// ```
/// use cocode_protocol::model::resolve_provider_type;
/// use cocode_protocol::ProviderType;
///
/// assert_eq!(resolve_provider_type("anthropic"), ProviderType::Anthropic);
/// assert_eq!(resolve_provider_type("openai"), ProviderType::Openai);
/// assert_eq!(resolve_provider_type("unknown"), ProviderType::OpenaiCompat);
/// ```
pub fn resolve_provider_type(provider: &str) -> ProviderType {
    // Normalize provider name to lowercase for comparison
    match provider.to_lowercase().as_str() {
        "anthropic" => ProviderType::Anthropic,
        "openai" => ProviderType::Openai,
        "gemini" | "genai" | "google" => ProviderType::Gemini,
        "volcengine" | "ark" => ProviderType::Volcengine,
        "zai" | "zhipu" | "zhipuai" => ProviderType::Zai,
        "openai_compat" | "openai-compat" => ProviderType::OpenaiCompat,
        // Default to OpenaiCompat for unknown providers (most compatible)
        _ => ProviderType::OpenaiCompat,
    }
}

/// Unified model specification: "{provider}/{model}" with resolved provider type.
///
/// Provides a single string format for specifying both provider and model,
/// along with the resolved `ProviderType` for API dispatch.
///
/// # Examples
///
/// ```
/// use cocode_protocol::model::ModelSpec;
/// use cocode_protocol::ProviderType;
///
/// let spec: ModelSpec = "anthropic/claude-opus-4".parse().unwrap();
/// assert_eq!(spec.provider, "anthropic");
/// assert_eq!(spec.model, "claude-opus-4");
/// assert_eq!(spec.provider_type, ProviderType::Anthropic);
/// assert_eq!(spec.to_string(), "anthropic/claude-opus-4");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelSpec {
    /// Provider name (e.g., "anthropic", "openai", "genai").
    pub provider: String,
    /// Resolved provider type for API dispatch.
    pub provider_type: ProviderType,
    /// Model ID (e.g., "claude-opus-4", "gpt-5").
    pub model: String,
}

impl ModelSpec {
    /// Create a new model specification with auto-resolved provider type.
    ///
    /// The provider type is automatically resolved from the provider name.
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        let provider = provider.into();
        let provider_type = resolve_provider_type(&provider);
        Self {
            provider,
            provider_type,
            model: model.into(),
        }
    }

    /// Create a new model specification with explicit provider type.
    ///
    /// Use this when you know the exact provider type and don't want
    /// to rely on string-based resolution.
    pub fn with_type(
        provider: impl Into<String>,
        provider_type: ProviderType,
        model: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            provider_type,
            model: model.into(),
        }
    }
}

impl fmt::Display for ModelSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
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
        // new() automatically resolves provider_type from provider string
        Ok(Self::new(parts[0], parts[1]))
    }
}

impl From<(String, ProviderType, String)> for ModelSpec {
    fn from((provider, provider_type, model): (String, ProviderType, String)) -> Self {
        Self::with_type(provider, provider_type, model)
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
