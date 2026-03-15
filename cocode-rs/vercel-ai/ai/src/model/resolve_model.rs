//! Model resolution functions.
//!
//! This module provides functions for resolving model references to actual model instances.

use std::sync::Arc;

use vercel_ai_provider::AISdkError;
use vercel_ai_provider::EmbeddingModelV4;
use vercel_ai_provider::ImageModelV4;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::NoSuchModelError;
use vercel_ai_provider::ProviderV4;
use vercel_ai_provider::VideoModelV4;

use crate::provider::get_default_provider;

/// A reference to a language model.
///
/// This allows flexibility in how models are specified:
/// - As a string ID that will be resolved via the default provider
/// - As a pre-resolved V4 model trait object
#[derive(Clone)]
pub enum LanguageModel {
    /// A string model ID that will be resolved via the default provider.
    String(String),
    /// A pre-resolved V4 model trait object.
    V4(Arc<dyn LanguageModelV4>),
}

impl Default for LanguageModel {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl LanguageModel {
    /// Create a new language model reference from a string ID.
    pub fn from_id(id: impl Into<String>) -> Self {
        Self::String(id.into())
    }

    /// Create a new language model reference from a V4 model.
    pub fn from_v4(model: Arc<dyn LanguageModelV4>) -> Self {
        Self::V4(model)
    }

    /// Check if this is a string ID.
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    /// Check if this is a V4 model.
    pub fn is_v4(&self) -> bool {
        matches!(self, Self::V4(_))
    }

    /// Get the string ID if this is a string reference.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(id) => Some(id),
            Self::V4(_) => None,
        }
    }
}

impl From<String> for LanguageModel {
    fn from(id: String) -> Self {
        Self::String(id)
    }
}

impl From<&str> for LanguageModel {
    fn from(id: &str) -> Self {
        Self::String(id.to_string())
    }
}

impl From<Arc<dyn LanguageModelV4>> for LanguageModel {
    fn from(model: Arc<dyn LanguageModelV4>) -> Self {
        Self::V4(model)
    }
}

/// A reference to an embedding model.
#[derive(Clone)]
pub enum EmbeddingModel {
    /// A string model ID that will be resolved via the default provider.
    String(String),
    /// A pre-resolved V4 model trait object.
    V4(Arc<dyn EmbeddingModelV4>),
}

impl Default for EmbeddingModel {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl EmbeddingModel {
    /// Create a new embedding model reference from a string ID.
    pub fn from_id(id: impl Into<String>) -> Self {
        Self::String(id.into())
    }

    /// Create a new embedding model reference from a V4 model.
    pub fn from_v4(model: Arc<dyn EmbeddingModelV4>) -> Self {
        Self::V4(model)
    }
}

impl From<String> for EmbeddingModel {
    fn from(id: String) -> Self {
        Self::String(id)
    }
}

impl From<&str> for EmbeddingModel {
    fn from(id: &str) -> Self {
        Self::String(id.to_string())
    }
}

impl From<Arc<dyn EmbeddingModelV4>> for EmbeddingModel {
    fn from(model: Arc<dyn EmbeddingModelV4>) -> Self {
        Self::V4(model)
    }
}

/// A reference to an image model.
#[derive(Clone)]
pub enum ImageModelRef {
    /// A string model ID that will be resolved via the default provider.
    String(String),
    /// A pre-resolved V4 model trait object.
    V4(Arc<dyn ImageModelV4>),
}

impl Default for ImageModelRef {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl ImageModelRef {
    /// Create a new image model reference from a string ID.
    pub fn from_id(id: impl Into<String>) -> Self {
        Self::String(id.into())
    }

    /// Create a new image model reference from a V4 model.
    pub fn from_v4(model: Arc<dyn ImageModelV4>) -> Self {
        Self::V4(model)
    }
}

impl From<String> for ImageModelRef {
    fn from(id: String) -> Self {
        Self::String(id)
    }
}

impl From<&str> for ImageModelRef {
    fn from(id: &str) -> Self {
        Self::String(id.to_string())
    }
}

impl From<Arc<dyn ImageModelV4>> for ImageModelRef {
    fn from(model: Arc<dyn ImageModelV4>) -> Self {
        Self::V4(model)
    }
}

/// A reference to a video model.
#[derive(Clone)]
pub enum VideoModelRef {
    /// A string model ID that will be resolved via the default provider.
    String(String),
    /// A pre-resolved V4 model trait object.
    V4(Arc<dyn VideoModelV4>),
}

impl Default for VideoModelRef {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl VideoModelRef {
    /// Create a new video model reference from a string ID.
    pub fn from_id(id: impl Into<String>) -> Self {
        Self::String(id.into())
    }

    /// Create a new video model reference from a V4 model.
    pub fn from_v4(model: Arc<dyn VideoModelV4>) -> Self {
        Self::V4(model)
    }
}

impl From<String> for VideoModelRef {
    fn from(id: String) -> Self {
        Self::String(id)
    }
}

impl From<&str> for VideoModelRef {
    fn from(id: &str) -> Self {
        Self::String(id.to_string())
    }
}

impl From<Arc<dyn VideoModelV4>> for VideoModelRef {
    fn from(model: Arc<dyn VideoModelV4>) -> Self {
        Self::V4(model)
    }
}

/// Resolve a language model reference to an actual model instance.
///
/// # Arguments
///
/// * `model` - The model reference (either a string ID or a V4 model)
///
/// # Errors
///
/// Returns an error if:
/// - A string ID is provided but no default provider is set
/// - The model ID is not found in the default provider
///
/// # Example
///
/// ```ignore
/// use vercel_ai::{resolve_language_model, LanguageModel, set_default_provider};
///
/// // With a pre-resolved model
/// let model = resolve_language_model(LanguageModel::V4(my_model))?;
///
/// // With a string ID (requires default provider)
/// set_default_provider(my_provider);
/// let model = resolve_language_model(LanguageModel::from_id("claude-3-sonnet"))?;
/// ```
pub fn resolve_language_model(
    model: LanguageModel,
) -> Result<Arc<dyn LanguageModelV4>, AISdkError> {
    match model {
        LanguageModel::V4(m) => Ok(m),
        LanguageModel::String(id) => {
            let provider = get_default_provider().ok_or_else(|| {
                AISdkError::new(
                    "No default provider set. Call set_default_provider() first or use a LanguageModel::V4 variant.",
                )
            })?;
            provider
                .language_model(&id)
                .map_err(|e| AISdkError::new(e.to_string()))
        }
    }
}

/// Resolve a language model reference with a provider.
///
/// This is useful when you have a specific provider and want to resolve
/// a string model ID against it.
pub fn resolve_language_model_with_provider(
    model: LanguageModel,
    provider: &dyn ProviderV4,
) -> Result<Arc<dyn LanguageModelV4>, NoSuchModelError> {
    match model {
        LanguageModel::V4(m) => Ok(m),
        LanguageModel::String(id) => provider.language_model(&id),
    }
}

/// Resolve an embedding model reference to an actual model instance.
pub fn resolve_embedding_model(
    model: EmbeddingModel,
) -> Result<Arc<dyn EmbeddingModelV4>, AISdkError> {
    match model {
        EmbeddingModel::V4(m) => Ok(m),
        EmbeddingModel::String(id) => {
            let provider = get_default_provider().ok_or_else(|| {
                AISdkError::new(
                    "No default provider set. Call set_default_provider() first or use an EmbeddingModel::V4 variant.",
                )
            })?;
            provider
                .embedding_model(&id)
                .map_err(|e| AISdkError::new(e.to_string()))
        }
    }
}

/// Resolve an embedding model reference with a provider.
pub fn resolve_embedding_model_with_provider(
    model: EmbeddingModel,
    provider: &dyn ProviderV4,
) -> Result<Arc<dyn EmbeddingModelV4>, NoSuchModelError> {
    match model {
        EmbeddingModel::V4(m) => Ok(m),
        EmbeddingModel::String(id) => provider.embedding_model(&id),
    }
}

/// Resolve an image model reference to an actual model instance.
pub fn resolve_image_model(model: ImageModelRef) -> Result<Arc<dyn ImageModelV4>, AISdkError> {
    match model {
        ImageModelRef::V4(m) => Ok(m),
        ImageModelRef::String(id) => {
            let provider = get_default_provider().ok_or_else(|| {
                AISdkError::new(
                    "No default provider set. Call set_default_provider() first or use an ImageModelRef::V4 variant.",
                )
            })?;
            provider
                .image_model(&id)
                .map_err(|e| AISdkError::new(e.to_string()))
        }
    }
}

/// Resolve an image model reference with a provider.
pub fn resolve_image_model_with_provider(
    model: ImageModelRef,
    provider: &dyn ProviderV4,
) -> Result<Arc<dyn ImageModelV4>, NoSuchModelError> {
    match model {
        ImageModelRef::V4(m) => Ok(m),
        ImageModelRef::String(id) => provider.image_model(&id),
    }
}

/// Resolve a video model reference to an actual model instance.
pub fn resolve_video_model(model: VideoModelRef) -> Result<Arc<dyn VideoModelV4>, AISdkError> {
    match model {
        VideoModelRef::V4(m) => Ok(m),
        VideoModelRef::String(id) => {
            let provider = get_default_provider().ok_or_else(|| {
                AISdkError::new(
                    "No default provider set. Call set_default_provider() first or use a VideoModelRef::V4 variant.",
                )
            })?;
            provider
                .video_model(&id)
                .map_err(|e| AISdkError::new(e.to_string()))
        }
    }
}

/// Resolve a video model reference with a provider.
pub fn resolve_video_model_with_provider(
    model: VideoModelRef,
    provider: &dyn ProviderV4,
) -> Result<Arc<dyn VideoModelV4>, NoSuchModelError> {
    match model {
        VideoModelRef::V4(m) => Ok(m),
        VideoModelRef::String(id) => provider.video_model(&id),
    }
}

#[cfg(test)]
#[path = "resolve_model.test.rs"]
mod tests;
