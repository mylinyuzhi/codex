//! Transcribe audio to text.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::TranscriptionFormat as ProviderTranscriptionFormat;
use vercel_ai_provider::TranscriptionModelV4;
use vercel_ai_provider::TranscriptionModelV4CallOptions;

use crate::error::AIError;
use crate::provider::get_default_provider;

use super::transcribe_result::TranscriptionFormat;
use super::transcribe_result::TranscriptionResult;
use super::transcribe_result::TranscriptionSegment;

/// A reference to a transcription model.
#[derive(Clone)]
pub enum TranscriptionModel {
    /// A string model ID that will be resolved via the default provider.
    String(String),
    /// A pre-resolved transcription model.
    V4(Arc<dyn TranscriptionModelV4>),
}

impl Default for TranscriptionModel {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl TranscriptionModel {
    /// Create from a string ID.
    pub fn from_id(id: impl Into<String>) -> Self {
        Self::String(id.into())
    }

    /// Create from a V4 model.
    pub fn from_v4(model: Arc<dyn TranscriptionModelV4>) -> Self {
        Self::V4(model)
    }

    /// Check if this is a string ID.
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }
}

impl From<String> for TranscriptionModel {
    fn from(id: String) -> Self {
        Self::String(id)
    }
}

impl From<&str> for TranscriptionModel {
    fn from(id: &str) -> Self {
        Self::String(id.to_string())
    }
}

impl From<Arc<dyn TranscriptionModelV4>> for TranscriptionModel {
    fn from(model: Arc<dyn TranscriptionModelV4>) -> Self {
        Self::V4(model)
    }
}

/// Audio data for transcription.
#[derive(Debug, Clone)]
pub enum AudioData {
    /// Raw audio bytes.
    Bytes(Vec<u8>),
    /// URL to an audio file.
    Url(String),
}

impl Default for AudioData {
    fn default() -> Self {
        Self::Bytes(Vec::new())
    }
}

impl AudioData {
    /// Create from bytes.
    pub fn bytes(data: Vec<u8>) -> Self {
        Self::Bytes(data)
    }

    /// Create from a URL.
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }
}

impl From<Vec<u8>> for AudioData {
    fn from(data: Vec<u8>) -> Self {
        Self::Bytes(data)
    }
}

impl From<&[u8]> for AudioData {
    fn from(data: &[u8]) -> Self {
        Self::Bytes(data.to_vec())
    }
}

/// Options for `transcribe`.
#[derive(Default)]
pub struct TranscribeOptions {
    /// The transcription model to use.
    pub model: TranscriptionModel,
    /// The audio data to transcribe.
    pub audio: AudioData,
    /// The language of the audio (ISO-639-1 code).
    pub language: Option<String>,
    /// A prompt to guide transcription.
    pub prompt: Option<String>,
    /// The response format.
    pub response_format: Option<TranscriptionFormat>,
    /// Abort signal for cancellation.
    pub abort_signal: Option<CancellationToken>,
}

impl TranscribeOptions {
    /// Create new options with a model and audio data.
    pub fn new(model: impl Into<TranscriptionModel>, audio: AudioData) -> Self {
        Self {
            model: model.into(),
            audio,
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

    /// Set the abort signal.
    pub fn with_abort_signal(mut self, signal: CancellationToken) -> Self {
        self.abort_signal = Some(signal);
        self
    }
}

/// Resolve a transcription model reference to an actual model instance.
fn resolve_transcription_model(
    model: TranscriptionModel,
) -> Result<Arc<dyn TranscriptionModelV4>, AIError> {
    match model {
        TranscriptionModel::V4(m) => Ok(m),
        TranscriptionModel::String(id) => {
            let provider = get_default_provider().ok_or_else(|| {
                AIError::InvalidArgument(
                    "No default provider set. Call set_default_provider() first or use a TranscriptionModel::V4 variant.".to_string(),
                )
            })?;
            provider
                .transcription_model(&id)
                .map_err(|e| AIError::ProviderError(AISdkError::new(e.to_string())))
        }
    }
}

/// Transcribe audio to text.
///
/// This function converts audio to text using a speech-to-text model.
///
/// # Arguments
///
/// * `options` - The transcription options including model and audio data.
///
/// # Returns
///
/// A `TranscriptionResult` containing the transcribed text and metadata.
///
/// # Example
///
/// ```ignore
/// use vercel_ai::{transcribe, TranscribeOptions, AudioData};
///
/// // From bytes
/// let audio_bytes = std::fs::read("audio.mp3")?;
/// let result = transcribe(TranscribeOptions {
///     model: "whisper-1".into(),
///     audio: AudioData::bytes(audio_bytes),
///     language: Some("en".to_string()),
///     ..Default::default()
/// }).await?;
///
/// println!("Transcription: {}", result.text);
/// ```
pub async fn transcribe(options: TranscribeOptions) -> Result<TranscriptionResult, AIError> {
    let model = resolve_transcription_model(options.model)?;
    let model_id = model.model_id().to_string();

    // Get audio data
    let audio_data = match options.audio {
        AudioData::Bytes(data) => data,
        AudioData::Url(_url) => {
            // For now, return an error for URLs
            // In a full implementation, we would download the audio
            return Err(AIError::InvalidArgument(
                "URL audio sources are not yet supported. Please download the audio and pass the bytes directly.".to_string(),
            ));
        }
    };

    // Detect content type from audio data (simple detection)
    let content_type = detect_audio_content_type(&audio_data);

    // Build call options
    let mut call_options = TranscriptionModelV4CallOptions::new(audio_data, content_type);

    // Set language
    if let Some(language) = options.language {
        call_options = call_options.with_language(language);
    }

    // Set prompt
    if let Some(prompt) = options.prompt {
        call_options = call_options.with_prompt(prompt);
    }

    // Set response format
    if let Some(format) = options.response_format {
        call_options = call_options.with_response_format(match format {
            TranscriptionFormat::Text => ProviderTranscriptionFormat::Text,
            TranscriptionFormat::Json => ProviderTranscriptionFormat::Json,
            TranscriptionFormat::Srt => ProviderTranscriptionFormat::Srt,
            TranscriptionFormat::VerboseJson => ProviderTranscriptionFormat::VerboseJson,
            TranscriptionFormat::Vtt => ProviderTranscriptionFormat::Vtt,
        });
    }

    // Set abort signal
    if let Some(signal) = options.abort_signal {
        call_options = call_options.with_abort_signal(signal);
    }

    // Call the model
    let result = model.do_transcribe(call_options).await?;

    // Check if text was generated
    if result.text.is_empty() {
        return Err(AIError::NoTranscriptGenerated);
    }

    // Build the result
    let mut transcription_result = TranscriptionResult::new(result.text, model_id);

    // Set language
    if let Some(language) = result.language {
        transcription_result = transcription_result.with_language(language);
    }

    // Set duration
    if let Some(duration) = result.duration {
        transcription_result = transcription_result.with_duration(duration);
    }

    // Convert segments
    if let Some(segments) = result.segments {
        let converted: Vec<TranscriptionSegment> = segments
            .into_iter()
            .map(|s| TranscriptionSegment::new(s.id, s.start, s.end, s.text))
            .collect();
        transcription_result = transcription_result.with_segments(converted);
    }

    Ok(transcription_result)
}

/// Detect audio content type from magic bytes.
fn detect_audio_content_type(data: &[u8]) -> String {
    // MP3: ID3 tag or MPEG audio frame sync
    if data.len() >= 3 && &data[0..3] == b"ID3" {
        return "audio/mpeg".to_string();
    }
    if data.len() >= 2 && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0 {
        return "audio/mpeg".to_string();
    }

    // WAV: RIFF...WAVE
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
        return "audio/wav".to_string();
    }

    // OGG: OggS
    if data.len() >= 4 && &data[0..4] == b"OggS" {
        return "audio/ogg".to_string();
    }

    // FLAC: fLaC
    if data.len() >= 4 && &data[0..4] == b"fLaC" {
        return "audio/flac".to_string();
    }

    // M4A/MP4: ftyp
    if data.len() >= 8 && &data[4..8] == b"ftyp" {
        return "audio/mp4".to_string();
    }

    // WebM
    if data.len() >= 4 && &data[0..4] == b"\x1a\x45\xdf\xa3" {
        return "audio/webm".to_string();
    }

    // Default to wav
    "audio/wav".to_string()
}

#[cfg(test)]
#[path = "transcribe.test.rs"]
mod tests;
