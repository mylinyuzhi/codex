//! Google Generative AI image model implementation.
//!
//! Supports two strategies:
//! - **Gemini models** (model_id starts with "gemini"): use the language model API with
//!   `responseModalities: ['IMAGE']`
//! - **Imagen models**: use the `:predict` endpoint

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use serde_json::json;

use vercel_ai_provider::AISdkError;
use vercel_ai_provider::GeneratedImage;
use vercel_ai_provider::ImageModelV4;
use vercel_ai_provider::ImageModelV4CallOptions;
use vercel_ai_provider::ImageModelV4GenerateResult;
use vercel_ai_provider::image_model::v4::ImageModelV4Response;

use vercel_ai_provider_utils::JsonResponseHandler;
use vercel_ai_provider_utils::combine_headers;
use vercel_ai_provider_utils::post_json_to_api_with_client;
use vercel_ai_provider_utils::without_trailing_slash;

use crate::get_model_path::get_model_path;
use crate::google_error::GoogleFailedResponseHandler;
use crate::google_generative_ai_image_settings::GoogleGenerativeAIImageSettings;

/// Configuration for the Google Generative AI image model.
pub struct GoogleGenerativeAIImageModelConfig {
    /// Provider identifier string.
    pub provider: String,
    /// Base URL for the API.
    pub base_url: String,
    /// Function to generate request headers.
    pub headers: Arc<dyn Fn() -> HashMap<String, String> + Send + Sync>,
    /// Optional HTTP client for connection pooling.
    pub client: Option<Arc<reqwest::Client>>,
}

/// Google Generative AI image model.
///
/// Generates images using either the Gemini language model API (for `gemini-*` models)
/// or the Imagen predict API (for `imagen-*` models).
pub struct GoogleGenerativeAIImageModel {
    model_id: String,
    settings: GoogleGenerativeAIImageSettings,
    config: GoogleGenerativeAIImageModelConfig,
}

impl GoogleGenerativeAIImageModel {
    /// Create a new Google Generative AI image model.
    pub fn new(
        model_id: impl Into<String>,
        settings: GoogleGenerativeAIImageSettings,
        config: GoogleGenerativeAIImageModelConfig,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            settings,
            config,
        }
    }

    /// Check if this is a Gemini model (uses language model API for images).
    fn is_gemini_model(&self) -> bool {
        self.model_id.starts_with("gemini")
    }

    /// Generate images using the Gemini language model API.
    async fn generate_gemini(
        &self,
        options: &ImageModelV4CallOptions,
    ) -> Result<ImageModelV4GenerateResult, AISdkError> {
        let model_path = get_model_path(&self.model_id);
        let url = format!(
            "{}/v1beta/{}:generateContent",
            without_trailing_slash(&self.config.base_url),
            model_path
        );

        let headers = combine_headers(vec![Some((self.config.headers)()), options.headers.clone()]);

        let n = options.n.unwrap_or(1);

        let mut generation_config = json!({
            "responseModalities": ["IMAGE"],
        });

        if let Some(ref aspect_ratio) = options.aspect_ratio {
            generation_config["imageGenerationConfig"] = json!({
                "aspectRatio": aspect_ratio,
            });
        }

        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": options.prompt }]
            }],
            "generationConfig": generation_config,
        });

        let mut images = Vec::new();
        for _ in 0..n {
            let response: Value = post_json_to_api_with_client(
                &url,
                Some(headers.clone()),
                &body,
                JsonResponseHandler::new(),
                GoogleFailedResponseHandler,
                options.abort_signal.clone(),
                self.config.client.clone(),
            )
            .await?;

            // Extract image from response
            if let Some(candidates) = response.get("candidates").and_then(|c| c.as_array()) {
                for candidate in candidates {
                    if let Some(parts) = candidate
                        .get("content")
                        .and_then(|c| c.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        for part in parts {
                            if let Some(inline_data) = part.get("inlineData")
                                && let Some(data) = inline_data.get("data").and_then(|d| d.as_str())
                            {
                                images.push(GeneratedImage::base64(data));
                            }
                        }
                    }
                }
            }
        }

        Ok(ImageModelV4GenerateResult {
            images,
            warnings: Vec::new(),
            provider_metadata: None,
            response: ImageModelV4Response::default(),
            usage: None,
        })
    }

    /// Generate images using the Imagen predict API.
    async fn generate_imagen(
        &self,
        options: &ImageModelV4CallOptions,
    ) -> Result<ImageModelV4GenerateResult, AISdkError> {
        let model_path = get_model_path(&self.model_id);
        let url = format!(
            "{}/v1beta/{}:predict",
            without_trailing_slash(&self.config.base_url),
            model_path
        );

        let headers = combine_headers(vec![Some((self.config.headers)()), options.headers.clone()]);

        let n = options.n.unwrap_or(1);

        let mut parameters = json!({
            "sampleCount": n,
        });

        if let Some(ref aspect_ratio) = options.aspect_ratio {
            parameters["aspectRatio"] = json!(aspect_ratio);
        }

        if let Some(seed) = options.seed {
            parameters["seed"] = json!(seed);
        }

        let body = json!({
            "instances": [{ "prompt": options.prompt }],
            "parameters": parameters,
        });

        let response: Value = post_json_to_api_with_client(
            &url,
            Some(headers),
            &body,
            JsonResponseHandler::new(),
            GoogleFailedResponseHandler,
            options.abort_signal.clone(),
            self.config.client.clone(),
        )
        .await?;

        let mut images = Vec::new();

        if let Some(predictions) = response.get("predictions").and_then(|p| p.as_array()) {
            for prediction in predictions {
                if let Some(data) = prediction
                    .get("bytesBase64Encoded")
                    .and_then(|d| d.as_str())
                {
                    images.push(GeneratedImage::base64(data));
                }
            }
        }

        Ok(ImageModelV4GenerateResult {
            images,
            warnings: Vec::new(),
            provider_metadata: None,
            response: ImageModelV4Response::default(),
            usage: None,
        })
    }
}

#[async_trait]
impl ImageModelV4 for GoogleGenerativeAIImageModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_images_per_call(&self) -> usize {
        if let Some(max) = self.settings.max_images_per_call {
            return max;
        }
        if self.is_gemini_model() { 10 } else { 4 }
    }

    async fn do_generate(
        &self,
        options: ImageModelV4CallOptions,
    ) -> Result<ImageModelV4GenerateResult, AISdkError> {
        if self.is_gemini_model() {
            self.generate_gemini(&options).await
        } else {
            self.generate_imagen(&options).await
        }
    }
}

#[cfg(test)]
#[path = "google_generative_ai_image_model.test.rs"]
mod tests;
