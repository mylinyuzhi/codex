use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use vercel_ai_provider::image_model::v4::{ImageModelV4Response, ImageModelV4Usage};
use vercel_ai_provider::{
    AISdkError, GeneratedImage, ImageData, ImageModelV4, ImageModelV4CallOptions,
    ImageModelV4GenerateResult, ImageSize,
};
use vercel_ai_provider_utils::{JsonResponseHandler, post_json_to_api_with_client};

use crate::openai_config::OpenAIConfig;
use crate::openai_error::OpenAIFailedResponseHandler;

use super::openai_image_api::OpenAIImageResponse;
use super::openai_image_options::extract_image_options;

/// OpenAI Image model (DALL-E, gpt-image-1, etc.).
pub struct OpenAIImageModel {
    model_id: String,
    config: Arc<OpenAIConfig>,
}

impl OpenAIImageModel {
    pub fn new(model_id: impl Into<String>, config: Arc<OpenAIConfig>) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }
}

#[async_trait]
impl ImageModelV4 for OpenAIImageModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_images_per_call(&self) -> usize {
        10
    }

    async fn do_generate(
        &self,
        options: ImageModelV4CallOptions,
    ) -> Result<ImageModelV4GenerateResult, AISdkError> {
        let openai_opts = extract_image_options(&options.provider_options);

        let size_str = options
            .size
            .as_ref()
            .map(|s| match s {
                ImageSize::S256x256 => "256x256",
                ImageSize::S512x512 => "512x512",
                ImageSize::S1024x1024 => "1024x1024",
                ImageSize::S1792x1024 => "1792x1024",
                ImageSize::S1024x1792 => "1024x1792",
                ImageSize::Custom { width, height } => {
                    // Can't return a &str for dynamic values, use leaked string
                    // This is acceptable for a limited set of sizes
                    Box::leak(format!("{width}x{height}").into_boxed_str()) as &str
                }
            })
            .or(openai_opts.size.as_deref());

        let mut body = json!({
            "model": self.model_id,
            "prompt": options.prompt,
            "response_format": "b64_json",
        });

        if let Some(n) = options.n {
            body["n"] = json!(n);
        }
        if let Some(size) = size_str {
            body["size"] = serde_json::Value::String(size.into());
        }
        if let Some(ref quality) = openai_opts.quality {
            body["quality"] = serde_json::Value::String(quality.clone());
        }
        if let Some(ref style) = openai_opts.style {
            body["style"] = serde_json::Value::String(style.clone());
        }
        if let Some(ref user) = openai_opts.user {
            body["user"] = serde_json::Value::String(user.clone());
        }

        let url = self.config.url("/images/generations");
        let headers = self.config.get_headers();

        let response: OpenAIImageResponse = post_json_to_api_with_client(
            &url,
            Some(headers),
            &body,
            JsonResponseHandler::new(),
            OpenAIFailedResponseHandler,
            options.abort_signal,
            self.config.client.clone(),
        )
        .await?;

        let images: Vec<GeneratedImage> = response
            .data
            .into_iter()
            .filter_map(|d| {
                let data = if let Some(b64) = d.b64_json {
                    ImageData::Base64(b64)
                } else if let Some(url) = d.url {
                    ImageData::Url(url)
                } else {
                    return None;
                };
                Some(GeneratedImage {
                    data,
                    media_type: Some("image/png".into()),
                })
            })
            .collect();

        let usage = response.usage.map(|u| ImageModelV4Usage {
            prompt_tokens: u.input_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
        });

        let timestamp = response
            .created
            .and_then(|ts| chrono::DateTime::from_timestamp(ts as i64, 0))
            .map(|dt| dt.to_rfc3339());

        Ok(ImageModelV4GenerateResult {
            images,
            warnings: Vec::new(),
            provider_metadata: None,
            response: ImageModelV4Response {
                timestamp,
                model_id: Some(self.model_id.clone()),
                headers: None,
            },
            usage,
        })
    }
}

#[cfg(test)]
#[path = "openai_image_model.test.rs"]
mod tests;
