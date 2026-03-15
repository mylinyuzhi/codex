//! Generate images from text prompts.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::ImageModelV4;
use vercel_ai_provider::ImageModelV4CallOptions;
use vercel_ai_provider::ImageSize as ProviderImageSize;

use crate::error::AIError;
use crate::provider::get_default_provider;

use super::image_result::GenerateImageResult;
use super::image_result::GeneratedImage;
use super::image_result::ImageSize;

/// A reference to an image model.
#[derive(Clone)]
pub enum ImageModel {
    /// A string model ID that will be resolved via the default provider.
    String(String),
    /// A pre-resolved image model.
    V4(Arc<dyn ImageModelV4>),
}

impl Default for ImageModel {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl ImageModel {
    /// Create from a string ID.
    pub fn from_id(id: impl Into<String>) -> Self {
        Self::String(id.into())
    }

    /// Create from a V4 model.
    pub fn from_v4(model: Arc<dyn ImageModelV4>) -> Self {
        Self::V4(model)
    }

    /// Check if this is a string ID.
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }
}

impl From<String> for ImageModel {
    fn from(id: String) -> Self {
        Self::String(id)
    }
}

impl From<&str> for ImageModel {
    fn from(id: &str) -> Self {
        Self::String(id.to_string())
    }
}

impl From<Arc<dyn ImageModelV4>> for ImageModel {
    fn from(model: Arc<dyn ImageModelV4>) -> Self {
        Self::V4(model)
    }
}

/// Prompt for image generation.
#[derive(Debug, Clone)]
pub enum ImagePrompt {
    /// A text prompt.
    Text(String),
    /// A prompt with reference images.
    WithImages {
        /// The text prompt.
        text: String,
        /// Reference images (URLs or base64 data).
        images: Vec<String>,
    },
}

impl Default for ImagePrompt {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl From<String> for ImagePrompt {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for ImagePrompt {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

/// Options for `generate_image`.
#[derive(Default)]
pub struct GenerateImageOptions {
    /// The image model to use.
    pub model: ImageModel,
    /// The prompt for image generation.
    pub prompt: ImagePrompt,
    /// Number of images to generate.
    pub n: Option<usize>,
    /// Maximum images per API call.
    pub max_images_per_call: Option<usize>,
    /// Size of the images.
    pub size: Option<ImageSize>,
    /// Abort signal for cancellation.
    pub abort_signal: Option<CancellationToken>,
}

impl GenerateImageOptions {
    /// Create new options with a model and prompt.
    pub fn new(model: impl Into<ImageModel>, prompt: impl Into<ImagePrompt>) -> Self {
        Self {
            model: model.into(),
            prompt: prompt.into(),
            ..Default::default()
        }
    }

    /// Set the number of images.
    pub fn with_n(mut self, n: usize) -> Self {
        self.n = Some(n);
        self
    }

    /// Set the size.
    pub fn with_size(mut self, size: ImageSize) -> Self {
        self.size = Some(size);
        self
    }

    /// Set the abort signal.
    pub fn with_abort_signal(mut self, signal: CancellationToken) -> Self {
        self.abort_signal = Some(signal);
        self
    }
}

/// Resolve an image model reference to an actual model instance.
fn resolve_image_model(model: ImageModel) -> Result<Arc<dyn ImageModelV4>, AIError> {
    match model {
        ImageModel::V4(m) => Ok(m),
        ImageModel::String(id) => {
            let provider = get_default_provider().ok_or_else(|| {
                AIError::InvalidArgument(
                    "No default provider set. Call set_default_provider() first or use an ImageModel::V4 variant.".to_string(),
                )
            })?;
            provider
                .image_model(&id)
                .map_err(|e| AIError::ProviderError(AISdkError::new(e.to_string())))
        }
    }
}

/// Generate images from a text prompt.
///
/// This function generates images using an image generation model.
///
/// # Arguments
///
/// * `options` - The generation options including model, prompt, and settings.
///
/// # Returns
///
/// A `GenerateImageResult` containing the generated images.
///
/// # Example
///
/// ```ignore
/// use vercel_ai::{generate_image, GenerateImageOptions, ImageSize};
///
/// let result = generate_image(GenerateImageOptions {
///     model: "dall-e-3".into(),
///     prompt: "A serene landscape with mountains".into(),
///     size: Some(ImageSize::S1024x1024),
///     ..Default::default()
/// }).await?;
///
/// // Get the first image
/// if let Some(image) = result.image() {
///     println!("Image URL: {:?}", image.as_url());
/// }
/// ```
pub async fn generate_image(options: GenerateImageOptions) -> Result<GenerateImageResult, AIError> {
    let model = resolve_image_model(options.model)?;
    let model_id = model.model_id().to_string();

    // Get the number of images to generate
    let n = options.n.unwrap_or(1);

    // Determine how many calls we need to make
    let max_per_call = options
        .max_images_per_call
        .unwrap_or_else(|| model.max_images_per_call());

    // Extract prompt text
    let prompt_text = match &options.prompt {
        ImagePrompt::Text(text) => text.clone(),
        ImagePrompt::WithImages { text, .. } => text.clone(),
    };

    // Build call options
    let mut call_options = ImageModelV4CallOptions::new(&prompt_text);

    // Set number of images
    call_options = call_options.with_n(n.min(max_per_call));

    // Set size
    if let Some(size) = options.size {
        call_options = call_options.with_size(match size {
            ImageSize::S256x256 => ProviderImageSize::S256x256,
            ImageSize::S512x512 => ProviderImageSize::S512x512,
            ImageSize::S1024x1024 => ProviderImageSize::S1024x1024,
            ImageSize::S1792x1024 => ProviderImageSize::S1792x1024,
            ImageSize::S1024x1792 => ProviderImageSize::S1024x1792,
            ImageSize::Custom { width, height } => ProviderImageSize::Custom { width, height },
        });
    }

    // Set abort signal
    if let Some(signal) = options.abort_signal {
        call_options = call_options.with_abort_signal(signal);
    }

    // Call the model
    let result = model.do_generate(call_options).await?;

    // Check if images were generated
    if result.images.is_empty() {
        return Err(AIError::NoImageGenerated);
    }

    // Convert results
    let images: Vec<GeneratedImage> = result
        .images
        .into_iter()
        .map(|img| {
            let generated = match img.data {
                vercel_ai_provider::ImageData::Url(url) => GeneratedImage::url(url),
                vercel_ai_provider::ImageData::Base64(data) => GeneratedImage::base64(data),
            };
            if let Some(mt) = img.media_type {
                generated.with_media_type(mt)
            } else {
                generated
            }
        })
        .collect();

    // Build the result
    let mut image_result = GenerateImageResult::new(images, model_id);

    if !result.warnings.is_empty() {
        image_result = image_result.with_warnings(result.warnings);
    }

    if let Some(usage) = result.usage {
        image_result = image_result.with_usage(usage);
    }

    Ok(image_result)
}

#[cfg(test)]
#[path = "generate_image.test.rs"]
mod tests;
