//! Provider-specific options.

pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod volcengine;
pub mod zai;

use serde::Deserialize;
use serde::Serialize;
use std::any::Any;
use std::fmt::Debug;

/// Trait for type-erased provider options.
///
/// This allows storing provider-specific options in a generic way
/// while still being able to downcast to the concrete type when needed.
pub trait ProviderOptionsData: Send + Sync + Debug + Any {
    /// Get a reference to the underlying Any type for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Clone the options into a box.
    fn clone_box(&self) -> Box<dyn ProviderOptionsData>;
}

/// Type-erased provider options.
pub type ProviderOptions = Box<dyn ProviderOptionsData>;

impl Clone for Box<dyn ProviderOptionsData> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

// Implement Serialize/Deserialize for ProviderOptions by serializing as empty object
impl Serialize for Box<dyn ProviderOptionsData> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Provider options are not serialized in the wire format
        serializer.serialize_none()
    }
}

impl<'de> Deserialize<'de> for Box<dyn ProviderOptionsData> {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Cannot deserialize type-erased options
        Err(serde::de::Error::custom(
            "cannot deserialize provider options directly",
        ))
    }
}

/// Helper to downcast provider options to a specific type.
pub fn downcast_options<T: ProviderOptionsData + 'static>(options: &ProviderOptions) -> Option<&T> {
    options.as_any().downcast_ref::<T>()
}

/// Configuration for extended thinking/reasoning.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThinkingConfig {
    /// Enable extended thinking.
    #[serde(default)]
    pub enabled: bool,
    /// Budget in tokens for thinking (Anthropic).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<i32>,
}

impl ThinkingConfig {
    /// Create a thinking config with a token budget.
    pub fn with_budget(tokens: i32) -> Self {
        Self {
            enabled: true,
            budget_tokens: Some(tokens),
        }
    }

    /// Create an enabled thinking config without explicit budget.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            budget_tokens: None,
        }
    }
}

// Re-export provider-specific options
pub use anthropic::AnthropicOptions;
pub use gemini::GeminiOptions;
pub use openai::OpenAIOptions;
pub use volcengine::ReasoningEffort;
pub use volcengine::VolcengineOptions;
pub use zai::ZaiOptions;
