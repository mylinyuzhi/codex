use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use vercel_ai_provider::AISdkError;
use vercel_ai_provider::SpeechModelV4;
use vercel_ai_provider::SpeechModelV4CallOptions;
use vercel_ai_provider::SpeechModelV4Request;
use vercel_ai_provider::SpeechModelV4Response;
use vercel_ai_provider::SpeechModelV4Result;

use crate::openai_config::OpenAIConfig;

/// OpenAI Speech (text-to-speech) model.
pub struct OpenAISpeechModel {
    model_id: String,
    config: Arc<OpenAIConfig>,
}

impl OpenAISpeechModel {
    pub fn new(model_id: impl Into<String>, config: Arc<OpenAIConfig>) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }
}

#[async_trait]
impl SpeechModelV4 for OpenAISpeechModel {
    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    async fn do_generate_speech(
        &self,
        options: SpeechModelV4CallOptions,
    ) -> Result<SpeechModelV4Result, AISdkError> {
        let voice = options.voice.unwrap_or_else(|| "alloy".into());

        let mut body = json!({
            "model": self.model_id,
            "input": options.text,
            "voice": voice,
        });

        if let Some(ref format) = options.output_format {
            body["response_format"] = serde_json::Value::String(format.clone());
        }
        if let Some(speed) = options.speed {
            body["speed"] = json!(speed);
        }
        if let Some(ref instructions) = options.instructions {
            body["instructions"] = serde_json::Value::String(instructions.clone());
        }

        let url = self.config.url("/audio/speech");
        let mut headers = self.config.get_headers();
        headers.insert("Content-Type".into(), "application/json".into());

        // Merge caller-supplied headers (caller overrides defaults).
        if let Some(extra) = options.headers {
            for (k, v) in extra {
                headers.insert(k, v);
            }
        }

        let client = self
            .config
            .client
            .as_ref()
            .map(|c| c.as_ref().clone())
            .unwrap_or_default();

        let mut builder = client.post(&url);
        for (k, v) in &headers {
            builder = builder.header(k, v);
        }
        builder = builder.json(&body);

        let response = builder
            .send()
            .await
            .map_err(|e| AISdkError::new(format!("OpenAI speech request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".into());
            return Err(AISdkError::new(format!(
                "OpenAI speech API error ({status}): {error_body}"
            )));
        }

        // Determine content type from response headers or from the requested format.
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| format_to_content_type(options.output_format.as_deref()));

        let audio = response.bytes().await.map_err(|e| {
            AISdkError::new(format!("Failed to read OpenAI speech response body: {e}"))
        })?;

        Ok(SpeechModelV4Result {
            audio: audio.to_vec(),
            content_type,
            warnings: Vec::new(),
            response: SpeechModelV4Response::default().with_model_id(self.model_id.clone()),
            request: Some(SpeechModelV4Request::default().with_body(body)),
            provider_metadata: None,
        })
    }
}

/// Map an OpenAI response_format string to a MIME content type.
fn format_to_content_type(format: Option<&str>) -> String {
    match format {
        Some("mp3") => "audio/mpeg".into(),
        Some("opus") => "audio/opus".into(),
        Some("aac") => "audio/aac".into(),
        Some("flac") => "audio/flac".into(),
        Some("wav") => "audio/wav".into(),
        Some("pcm") => "audio/pcm".into(),
        _ => "audio/mpeg".into(), // OpenAI default is mp3
    }
}

#[cfg(test)]
#[path = "openai_speech_model.test.rs"]
mod tests;
