//! Provider-specific options.
//!
//! This module provides type-erased provider options with optional
//! provider validation through the [`ProviderMarker`] trait.
//!
//! # Typed Options API
//!
//! For better IDE support and compile-time safety hints, use the typed
//! methods on [`GenerateRequest`](crate::GenerateRequest):
//!
//! ```
//! use hyper_sdk::{GenerateRequest, OpenAIOptions};
//! use hyper_sdk::options::openai::ReasoningEffort;
//!
//! let request = GenerateRequest::from_text("Hello")
//!     .with_openai_options(
//!         OpenAIOptions::new()
//!             .with_reasoning_effort(ReasoningEffort::High)
//!     );
//! ```

pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod volcengine;
pub mod zai;

use serde::Deserialize;
use serde::Serialize;
use std::any::Any;
use std::any::TypeId;
use std::fmt::Debug;

/// Marker trait for provider-specific options.
///
/// This trait associates options with their target provider name,
/// enabling runtime validation when options are passed to models.
///
/// # Example
///
/// ```
/// use hyper_sdk::options::{ProviderMarker, OpenAIOptions};
///
/// // OpenAI options are marked for "openai" provider
/// assert_eq!(OpenAIOptions::PROVIDER_NAME, "openai");
/// ```
pub trait ProviderMarker {
    /// The canonical provider name (e.g., "openai", "anthropic", "gemini").
    const PROVIDER_NAME: &'static str;
}

/// Combined trait for typed, provider-aware options.
///
/// Options implementing this trait can be validated at runtime
/// to ensure they're used with the correct provider.
pub trait TypedProviderOptions: ProviderOptionsData + ProviderMarker {}

/// Trait for type-erased provider options.
///
/// This allows storing provider-specific options in a generic way
/// while still being able to downcast to the concrete type when needed.
pub trait ProviderOptionsData: Send + Sync + Debug + Any {
    /// Get a reference to the underlying Any type for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Clone the options into a box.
    fn clone_box(&self) -> Box<dyn ProviderOptionsData>;

    /// Get the provider name if this type implements ProviderMarker.
    ///
    /// Returns `None` for options that don't implement ProviderMarker.
    fn provider_name(&self) -> Option<&'static str> {
        None
    }
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

use crate::error::HyperError;

/// Downcast provider options with explicit error on type mismatch.
///
/// Unlike `downcast_options()` which returns `Option`, this function
/// returns a `Result` with a descriptive error message on failure.
///
/// # Example
///
/// ```ignore
/// use hyper_sdk::{try_downcast_options, OpenAIOptions, HyperError};
///
/// let opts = OpenAIOptions::new().boxed();
/// let result = try_downcast_options::<OpenAIOptions>(&opts);
/// assert!(result.is_ok());
///
/// // Type mismatch produces a clear error
/// let result = try_downcast_options::<AnthropicOptions>(&opts);
/// assert!(matches!(result, Err(HyperError::ConfigError(_))));
/// ```
pub fn try_downcast_options<T: ProviderOptionsData + 'static>(
    options: &ProviderOptions,
) -> Result<&T, HyperError> {
    options.as_any().downcast_ref::<T>().ok_or_else(|| {
        HyperError::ConfigError(format!(
            "Provider options type mismatch: expected {}, got different type",
            std::any::type_name::<T>()
        ))
    })
}

/// Validate that provider options match the expected provider.
///
/// This function checks if the given options are appropriate for the
/// specified provider. On mismatch, it logs a warning but does not
/// return an error (for backward compatibility).
///
/// # Returns
///
/// - `Ok(true)` if options match the provider or no options are provided
/// - `Ok(false)` if options don't match (warning logged)
///
/// # Example
///
/// ```
/// use hyper_sdk::options::{validate_options_for_provider, OpenAIOptions, AnthropicOptions};
///
/// let opts = OpenAIOptions::new().boxed();
///
/// // Correct provider
/// assert!(validate_options_for_provider(Some(&opts), "openai").unwrap());
///
/// // Wrong provider - logs warning, returns false
/// assert!(!validate_options_for_provider(Some(&opts), "anthropic").unwrap());
/// ```
pub fn validate_options_for_provider(
    options: Option<&ProviderOptions>,
    provider: &str,
) -> Result<bool, HyperError> {
    let Some(opts) = options else {
        return Ok(true);
    };

    // Get the TypeId of the concrete type through the as_any() method
    let type_id = opts.as_any().type_id();

    // Check known option types
    let expected_provider = match () {
        _ if type_id == TypeId::of::<OpenAIOptions>() => "openai",
        _ if type_id == TypeId::of::<AnthropicOptions>() => "anthropic",
        _ if type_id == TypeId::of::<GeminiOptions>() => "gemini",
        _ if type_id == TypeId::of::<VolcengineOptions>() => "volcengine",
        _ if type_id == TypeId::of::<ZaiOptions>() => "zhipuai",
        _ => {
            // Unknown options type - allow for extensibility
            return Ok(true);
        }
    };

    if expected_provider == provider {
        Ok(true)
    } else {
        tracing::warn!(
            expected_provider = %expected_provider,
            actual_provider = %provider,
            "Provider options type mismatch - options will be ignored"
        );
        Ok(false)
    }
}

// Re-export provider-specific options
pub use anthropic::AnthropicOptions;
pub use gemini::GeminiOptions;
pub use openai::OpenAIOptions;
pub use volcengine::ReasoningEffort;
pub use volcengine::VolcengineOptions;
pub use zai::ZaiOptions;

/// Known provider names for use with options validation.
pub mod provider_names {
    /// OpenAI provider name.
    pub const OPENAI: &str = "openai";
    /// Anthropic provider name.
    pub const ANTHROPIC: &str = "anthropic";
    /// Google Gemini provider name.
    pub const GEMINI: &str = "gemini";
    /// Volcengine Ark provider name.
    pub const VOLCENGINE: &str = "volcengine";
    /// Z.AI / ZhipuAI provider name.
    pub const ZHIPUAI: &str = "zhipuai";
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
