//! Speech model trait and related types (V4).
//!
//! This module defines the `SpeechModelV4` trait for implementing text-to-speech models
//! that follow the Vercel AI SDK v4 specification.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

use crate::errors::AISdkError;
use crate::shared::ProviderOptions;

/// The speech model trait (V4).
///
/// This trait defines the interface for text-to-speech models following the
/// Vercel AI SDK v4 specification.
#[async_trait]
pub trait SpeechModelV4: Send + Sync {
    /// Get the specification version.
    fn specification_version(&self) -> &'static str {
        "v4"
    }

    /// Get the provider name.
    fn provider(&self) -> &str;

    /// Get the model ID.
    fn model_id(&self) -> &str;

    /// Generate speech from text.
    async fn do_generate_speech(
        &self,
        options: SpeechModelV4CallOptions,
    ) -> Result<SpeechModelV4Result, AISdkError>;
}

/// Options for a speech model call.
#[derive(Debug, Clone, Default)]
pub struct SpeechModelV4CallOptions {
    /// The text to convert to speech.
    pub text: String,
    /// The voice to use.
    pub voice: Option<SpeechVoice>,
    /// The output format.
    pub response_format: Option<SpeechFormat>,
    /// The speed of the speech (0.25 to 4.0).
    pub speed: Option<f32>,
    /// Provider-specific options.
    pub provider_options: Option<ProviderOptions>,
    /// Abort signal for cancellation.
    pub abort_signal: Option<CancellationToken>,
    /// Headers to include in the request.
    pub headers: Option<HashMap<String, String>>,
}

impl SpeechModelV4CallOptions {
    /// Create new call options.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }

    /// Set the voice.
    pub fn with_voice(mut self, voice: SpeechVoice) -> Self {
        self.voice = Some(voice);
        self
    }

    /// Set the response format.
    pub fn with_response_format(mut self, format: SpeechFormat) -> Self {
        self.response_format = Some(format);
        self
    }

    /// Set the speed.
    pub fn with_speed(mut self, speed: f32) -> Self {
        self.speed = Some(speed);
        self
    }

    /// Set provider options.
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Set the abort signal.
    pub fn with_abort_signal(mut self, signal: CancellationToken) -> Self {
        self.abort_signal = Some(signal);
        self
    }
}

/// The result of a speech generation call.
#[derive(Debug, Clone)]
pub struct SpeechModelV4Result {
    /// The audio data.
    pub audio: Vec<u8>,
    /// The MIME type of the audio.
    pub content_type: String,
}

impl SpeechModelV4Result {
    /// Create a new speech result.
    pub fn new(audio: Vec<u8>, content_type: impl Into<String>) -> Self {
        Self {
            audio,
            content_type: content_type.into(),
        }
    }

    /// Create from MP3 data.
    pub fn mp3(audio: Vec<u8>) -> Self {
        Self::new(audio, "audio/mpeg")
    }

    /// Create from WAV data.
    pub fn wav(audio: Vec<u8>) -> Self {
        Self::new(audio, "audio/wav")
    }
}

/// Voice options for speech synthesis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechVoice {
    /// The voice ID.
    pub id: String,
    /// The voice name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl SpeechVoice {
    /// Create a new voice.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: None,
        }
    }

    /// Set the voice name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// Audio format options for speech synthesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeechFormat {
    /// MP3 format.
    #[default]
    Mp3,
    /// Opus format.
    Opus,
    /// AAC format.
    Aac,
    /// FLAC format.
    Flac,
    /// WAV format.
    Wav,
    /// PCM format.
    Pcm,
}

#[cfg(test)]
#[path = "speech_model_v4.test.rs"]
mod tests;
