//! Google Generative AI video model implementation.
//!
//! Uses async polling: POST to `:predictLongRunning`, then poll GET until `done: true`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use serde_json::Value;
use serde_json::json;

use vercel_ai_provider::AISdkError;
use vercel_ai_provider::VideoModelV4;
use vercel_ai_provider::VideoModelV4CallOptions;
use vercel_ai_provider::VideoModelV4Result;
use vercel_ai_provider::video_model::v4::GeneratedVideo;
use vercel_ai_provider::video_model::v4::VideoData;

use vercel_ai_provider_utils::JsonResponseHandler;
use vercel_ai_provider_utils::combine_headers;
use vercel_ai_provider_utils::delay;
use vercel_ai_provider_utils::get_from_api_with_client;
use vercel_ai_provider_utils::post_json_to_api_with_client;
use vercel_ai_provider_utils::without_trailing_slash;

use crate::get_model_path::get_model_path;
use crate::google_error::GoogleFailedResponseHandler;

/// Configuration for the Google Generative AI video model.
pub struct GoogleGenerativeAIVideoModelConfig {
    /// Provider identifier string.
    pub provider: String,
    /// Base URL for the API.
    pub base_url: String,
    /// Function to generate request headers.
    pub headers: Arc<dyn Fn() -> HashMap<String, String> + Send + Sync>,
    /// Optional HTTP client for connection pooling.
    pub client: Option<Arc<reqwest::Client>>,
    /// Poll interval for long-running operations (default: 5 seconds).
    pub poll_interval: Option<Duration>,
    /// Maximum polling timeout (default: 5 minutes).
    pub poll_timeout: Option<Duration>,
}

/// Google Generative AI video model.
///
/// Generates videos using the Google Generative AI `:predictLongRunning` endpoint
/// with polling for completion.
pub struct GoogleGenerativeAIVideoModel {
    model_id: String,
    config: GoogleGenerativeAIVideoModelConfig,
}

impl GoogleGenerativeAIVideoModel {
    /// Create a new Google Generative AI video model.
    pub fn new(model_id: impl Into<String>, config: GoogleGenerativeAIVideoModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    fn poll_interval(&self) -> Duration {
        self.config.poll_interval.unwrap_or(Duration::from_secs(5))
    }

    fn poll_timeout(&self) -> Duration {
        self.config.poll_timeout.unwrap_or(Duration::from_secs(300))
    }
}

#[async_trait]
impl VideoModelV4 for GoogleGenerativeAIVideoModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    async fn do_generate_video(
        &self,
        options: VideoModelV4CallOptions,
    ) -> Result<VideoModelV4Result, AISdkError> {
        let model_path = get_model_path(&self.model_id);
        let url = format!(
            "{}/v1beta/{}:predictLongRunning",
            without_trailing_slash(&self.config.base_url),
            model_path
        );

        let headers = combine_headers(vec![Some((self.config.headers)()), options.headers.clone()]);

        let n = options.n.unwrap_or(1);

        // Build instance
        let mut instance = json!({
            "prompt": options.prompt,
        });

        // Add reference image if provided
        if let Some(ref image_bytes) = options.image {
            let base64_data = base64::engine::general_purpose::STANDARD.encode(image_bytes);
            instance["image"] = json!({
                "bytesBase64Encoded": base64_data,
            });
            if let Some(ref content_type) = options.image_content_type {
                instance["image"]["mimeType"] = json!(content_type);
            }
        }

        // Build parameters
        let mut parameters = json!({
            "sampleCount": n,
        });

        if let Some(ref style) = options.style {
            parameters["style"] = json!(style);
        }

        let body = json!({
            "instances": [instance],
            "parameters": parameters,
        });

        // Start the long-running operation
        let operation: Value = post_json_to_api_with_client(
            &url,
            Some(headers.clone()),
            &body,
            JsonResponseHandler::new(),
            GoogleFailedResponseHandler,
            options.abort_signal.clone(),
            self.config.client.clone(),
        )
        .await?;

        // Get operation name for polling
        let operation_name = operation
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| AISdkError::new("No operation name in response"))?
            .to_string();

        // Poll for completion
        let poll_url = format!(
            "{}/v1beta/{}",
            without_trailing_slash(&self.config.base_url),
            operation_name
        );

        let start = std::time::Instant::now();
        let timeout = self.poll_timeout();

        loop {
            if start.elapsed() > timeout {
                return Err(AISdkError::new(format!(
                    "Video generation timed out after {} seconds",
                    timeout.as_secs()
                )));
            }

            delay(self.poll_interval()).await;

            let status: Value = get_from_api_with_client(
                &poll_url,
                Some(headers.clone()),
                JsonResponseHandler::new(),
                GoogleFailedResponseHandler,
                options.abort_signal.clone(),
                self.config.client.clone(),
            )
            .await?;

            if status.get("done").and_then(Value::as_bool) == Some(true) {
                // Check for error
                if let Some(error) = status.get("error") {
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Video generation failed");
                    return Err(AISdkError::new(message));
                }

                // Extract videos from response
                let mut videos = Vec::new();
                if let Some(response) = status.get("response")
                    && let Some(predictions) =
                        response.get("predictions").and_then(|p| p.as_array())
                {
                    for prediction in predictions {
                        if let Some(data) = prediction
                            .get("bytesBase64Encoded")
                            .and_then(|d| d.as_str())
                        {
                            videos.push(GeneratedVideo {
                                data: VideoData::Base64(data.to_string()),
                                content_type: Some("video/mp4".to_string()),
                            });
                        }
                    }
                }

                return Ok(VideoModelV4Result { videos });
            }
        }
    }
}

#[cfg(test)]
#[path = "google_generative_ai_video_model.test.rs"]
mod tests;
