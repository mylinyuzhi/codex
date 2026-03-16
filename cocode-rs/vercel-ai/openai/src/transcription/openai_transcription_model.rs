use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use vercel_ai_provider::AISdkError;
use vercel_ai_provider::APICallError;
use vercel_ai_provider::TranscriptionModelV4;
use vercel_ai_provider::TranscriptionModelV4CallOptions;
use vercel_ai_provider::TranscriptionModelV4Request;
use vercel_ai_provider::TranscriptionModelV4Response;
use vercel_ai_provider::TranscriptionModelV4Result;
use vercel_ai_provider::TranscriptionSegmentV4;
use vercel_ai_provider_utils::FormData;

use crate::openai_config::OpenAIConfig;

/// Provider-specific options for OpenAI transcription models.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenAITranscriptionOptions {
    language: Option<String>,
    temperature: Option<f64>,
    prompt: Option<String>,
}

/// Extract transcription-specific options from provider options.
fn extract_transcription_options(
    provider_options: &Option<vercel_ai_provider::ProviderOptions>,
) -> OpenAITranscriptionOptions {
    provider_options
        .as_ref()
        .and_then(|opts| opts.0.get("openai"))
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value::<OpenAITranscriptionOptions>(v).ok())
        .unwrap_or_default()
}

/// Map a media type to a file extension for the multipart upload filename.
fn extension_from_media_type(media_type: &str) -> &str {
    match media_type {
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/mp3" | "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/m4a" => "m4a",
        "audio/webm" => "webm",
        "audio/ogg" => "ogg",
        "audio/flac" => "flac",
        _ => "bin",
    }
}

/// OpenAI API transcription response shape (verbose_json format).
#[derive(Debug, Deserialize)]
struct OpenAITranscriptionResponse {
    text: String,
    language: Option<String>,
    duration: Option<f64>,
    #[serde(default)]
    segments: Vec<OpenAITranscriptionSegment>,
}

/// A segment in the OpenAI transcription response.
#[derive(Debug, Deserialize)]
struct OpenAITranscriptionSegment {
    text: String,
    start: f64,
    end: f64,
}

/// OpenAI Transcription model.
///
/// Implements `TranscriptionModelV4` for OpenAI's audio transcription API.
pub struct OpenAITranscriptionModel {
    model_id: String,
    config: Arc<OpenAIConfig>,
}

impl OpenAITranscriptionModel {
    /// Create a new transcription model instance.
    pub fn new(model_id: impl Into<String>, config: Arc<OpenAIConfig>) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }
}

#[async_trait]
impl TranscriptionModelV4 for OpenAITranscriptionModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    async fn do_transcribe(
        &self,
        options: TranscriptionModelV4CallOptions,
    ) -> Result<TranscriptionModelV4Result, AISdkError> {
        let openai_opts = extract_transcription_options(&options.provider_options);

        let url = self.config.url("/audio/transcriptions");
        let config_headers = self.config.get_headers();

        // Build multipart form
        let ext = extension_from_media_type(&options.media_type);
        let filename = format!("audio.{ext}");

        let mut form = FormData::new()
            .bytes_with_mime("file", options.audio, &filename, &options.media_type)
            .text("model", self.model_id.clone())
            .text("response_format", "verbose_json")
            .text("timestamp_granularities[]", "segment");

        if let Some(ref language) = openai_opts.language {
            form = form.text("language", language.clone());
        }

        if let Some(temperature) = openai_opts.temperature {
            form = form.text("temperature", temperature.to_string());
        }

        if let Some(ref prompt) = openai_opts.prompt {
            form = form.text("prompt", prompt.clone());
        }

        // Build HTTP request
        let client = self
            .config
            .client
            .as_ref()
            .map(|c| c.as_ref().clone())
            .unwrap_or_default();

        let mut request = client.post(&url);

        // Apply config headers
        for (k, v) in &config_headers {
            request = request.header(k, v);
        }

        // Apply call-level headers
        if let Some(ref call_headers) = options.headers {
            for (k, v) in call_headers {
                request = request.header(k, v);
            }
        }

        let response = request.multipart(form.build()).send().await.map_err(|e| {
            AISdkError::new(format!("OpenAI transcription request failed: {e}")).with_cause(
                Box::new(APICallError::new(e.to_string(), &url).with_retryable(e.is_timeout())),
            )
        })?;

        let status = response.status();

        // Extract response headers before consuming the body
        let response_headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());

            let message = match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(json) => json
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or(&body)
                    .to_string(),
                Err(_) => body.clone(),
            };

            let is_retryable = status.as_u16() == 429 || status.as_u16() >= 500;

            return Err(
                AISdkError::new(format!("OpenAI API error ({status}): {message}")).with_cause(
                    Box::new(
                        APICallError::new(&message, &url)
                            .with_status(status.as_u16())
                            .with_response_body(&body)
                            .with_retryable(is_retryable),
                    ),
                ),
            );
        }

        let raw_body = response.text().await.map_err(|e| {
            AISdkError::new(format!("Failed to read transcription response body: {e}"))
        })?;

        let api_response: OpenAITranscriptionResponse = serde_json::from_str(&raw_body)
            .map_err(|e| AISdkError::new(format!("Failed to parse transcription response: {e}")))?;

        let segments: Vec<TranscriptionSegmentV4> = api_response
            .segments
            .into_iter()
            .map(|s| TranscriptionSegmentV4::new(s.text, s.start, s.end))
            .collect();

        let body_value: Option<serde_json::Value> = serde_json::from_str(&raw_body).ok();

        Ok(TranscriptionModelV4Result::new(api_response.text)
            .with_response(
                TranscriptionModelV4Response::default()
                    .with_model_id(self.model_id.clone())
                    .with_headers(response_headers)
                    .with_body(body_value.unwrap_or(serde_json::Value::Null)),
            )
            .with_request(TranscriptionModelV4Request::default())
            .with_segments(segments)
            .with_language_opt(api_response.language)
            .with_duration_opt(api_response.duration))
    }
}

/// Extension trait to add optional setters to `TranscriptionModelV4Result`.
trait TranscriptionResultExt {
    fn with_language_opt(self, language: Option<String>) -> Self;
    fn with_duration_opt(self, duration: Option<f64>) -> Self;
}

impl TranscriptionResultExt for TranscriptionModelV4Result {
    fn with_language_opt(self, language: Option<String>) -> Self {
        match language {
            Some(lang) => self.with_language(lang),
            None => self,
        }
    }

    fn with_duration_opt(self, duration: Option<f64>) -> Self {
        match duration {
            Some(d) => self.with_duration_in_seconds(d),
            None => self,
        }
    }
}

#[cfg(test)]
#[path = "openai_transcription_model.test.rs"]
mod tests;
