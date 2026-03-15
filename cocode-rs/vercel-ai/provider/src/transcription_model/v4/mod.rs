//! Transcription model trait and related types (V4).
//!
//! This module defines the `TranscriptionModelV4` trait for implementing speech-to-text models
//! that follow the Vercel AI SDK v4 specification.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

use crate::errors::AISdkError;
use crate::shared::ProviderOptions;

/// The transcription model trait (V4).
///
/// This trait defines the interface for speech-to-text models following the
/// Vercel AI SDK v4 specification.
#[async_trait]
pub trait TranscriptionModelV4: Send + Sync {
    /// Get the specification version.
    fn specification_version(&self) -> &'static str {
        "v4"
    }

    /// Get the provider name.
    fn provider(&self) -> &str;

    /// Get the model ID.
    fn model_id(&self) -> &str;

    /// Transcribe audio to text.
    async fn do_transcribe(
        &self,
        options: TranscriptionModelV4CallOptions,
    ) -> Result<TranscriptionModelV4Result, AISdkError>;
}

/// Options for a transcription model call.
#[derive(Debug, Clone, Default)]
pub struct TranscriptionModelV4CallOptions {
    /// The audio data to transcribe.
    pub audio: Vec<u8>,
    /// The MIME type of the audio.
    pub content_type: String,
    /// The language of the audio (ISO-639-1 code).
    pub language: Option<String>,
    /// The prompt to guide transcription.
    pub prompt: Option<String>,
    /// The response format.
    pub response_format: Option<TranscriptionFormat>,
    /// The temperature for sampling (0-1).
    pub temperature: Option<f32>,
    /// Whether to include timestamps.
    pub timestamp_granularities: Option<Vec<TimestampGranularity>>,
    /// Provider-specific options.
    pub provider_options: Option<ProviderOptions>,
    /// Abort signal for cancellation.
    pub abort_signal: Option<CancellationToken>,
    /// Headers to include in the request.
    pub headers: Option<HashMap<String, String>>,
}

impl TranscriptionModelV4CallOptions {
    /// Create new call options.
    pub fn new(audio: Vec<u8>, content_type: impl Into<String>) -> Self {
        Self {
            audio,
            content_type: content_type.into(),
            ..Default::default()
        }
    }

    /// Set the language.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Set the prompt.
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Set the response format.
    pub fn with_response_format(mut self, format: TranscriptionFormat) -> Self {
        self.response_format = Some(format);
        self
    }

    /// Set the temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
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

/// The result of a transcription call.
#[derive(Debug, Clone)]
pub struct TranscriptionModelV4Result {
    /// The transcribed text.
    pub text: String,
    /// The detected language.
    pub language: Option<String>,
    /// Duration of the audio in seconds.
    pub duration: Option<f32>,
    /// Word-level timestamps (if requested).
    pub words: Option<Vec<TranscriptionWord>>,
    /// Segment-level timestamps (if requested).
    pub segments: Option<Vec<TranscriptionSegment>>,
}

impl TranscriptionModelV4Result {
    /// Create a new transcription result.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            language: None,
            duration: None,
            words: None,
            segments: None,
        }
    }

    /// Set the language.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Set the duration.
    pub fn with_duration(mut self, duration: f32) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set the words.
    pub fn with_words(mut self, words: Vec<TranscriptionWord>) -> Self {
        self.words = Some(words);
        self
    }

    /// Set the segments.
    pub fn with_segments(mut self, segments: Vec<TranscriptionSegment>) -> Self {
        self.segments = Some(segments);
        self
    }
}

/// A transcribed word with timing information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionWord {
    /// The word text.
    pub word: String,
    /// Start time in seconds.
    pub start: f32,
    /// End time in seconds.
    pub end: f32,
}

impl TranscriptionWord {
    /// Create a new transcribed word.
    pub fn new(word: impl Into<String>, start: f32, end: f32) -> Self {
        Self {
            word: word.into(),
            start,
            end,
        }
    }
}

/// A transcribed segment with timing information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    /// The segment ID.
    pub id: usize,
    /// Start time in seconds.
    pub start: f32,
    /// End time in seconds.
    pub end: f32,
    /// The segment text.
    pub text: String,
    /// The tokens in the segment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Vec<i32>>,
    /// The temperature used for this segment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// The average log probability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_logprob: Option<f32>,
    /// The compression ratio.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression_ratio: Option<f32>,
    /// The no speech probability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_speech_prob: Option<f32>,
}

impl TranscriptionSegment {
    /// Create a new transcribed segment.
    pub fn new(id: usize, start: f32, end: f32, text: impl Into<String>) -> Self {
        Self {
            id,
            start,
            end,
            text: text.into(),
            tokens: None,
            temperature: None,
            avg_logprob: None,
            compression_ratio: None,
            no_speech_prob: None,
        }
    }
}

/// Transcription response format options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionFormat {
    /// Simple text format.
    #[default]
    Text,
    /// JSON format with metadata.
    Json,
    /// SubRip subtitle format.
    Srt,
    /// Verbose JSON with timestamps.
    VerboseJson,
    /// WebVTT format.
    Vtt,
}

/// Timestamp granularity options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimestampGranularity {
    /// Word-level timestamps.
    Word,
    /// Segment-level timestamps.
    Segment,
}

#[cfg(test)]
#[path = "transcription_model_v4.test.rs"]
mod tests;
