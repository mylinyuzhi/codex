//! Generate speech from text.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::SpeechFormat as ProviderSpeechFormat;
use vercel_ai_provider::SpeechModelV4;
use vercel_ai_provider::SpeechModelV4CallOptions;
use vercel_ai_provider::SpeechVoice as ProviderSpeechVoice;

use crate::error::AIError;
use crate::provider::get_default_provider;

use super::speech_result::GeneratedAudioFile;
use super::speech_result::SpeechFormat;
use super::speech_result::SpeechResult;
use super::speech_result::SpeechVoice;

/// A reference to a speech model.
#[derive(Clone)]
pub enum SpeechModel {
    /// A string model ID that will be resolved via the default provider.
    String(String),
    /// A pre-resolved speech model.
    V4(Arc<dyn SpeechModelV4>),
}

impl Default for SpeechModel {
    fn default() -> Self {
        Self::String(String::new())
    }
}

impl SpeechModel {
    /// Create from a string ID.
    pub fn from_id(id: impl Into<String>) -> Self {
        Self::String(id.into())
    }

    /// Create from a V4 model.
    pub fn from_v4(model: Arc<dyn SpeechModelV4>) -> Self {
        Self::V4(model)
    }

    /// Check if this is a string ID.
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }
}

impl From<String> for SpeechModel {
    fn from(id: String) -> Self {
        Self::String(id)
    }
}

impl From<&str> for SpeechModel {
    fn from(id: &str) -> Self {
        Self::String(id.to_string())
    }
}

impl From<Arc<dyn SpeechModelV4>> for SpeechModel {
    fn from(model: Arc<dyn SpeechModelV4>) -> Self {
        Self::V4(model)
    }
}

/// Options for `generate_speech`.
#[derive(Default)]
pub struct GenerateSpeechOptions {
    /// The speech model to use.
    pub model: SpeechModel,
    /// The text to convert to speech.
    pub text: String,
    /// The voice to use for speech generation.
    pub voice: Option<SpeechVoice>,
    /// The output format (e.g., "mp3", "wav").
    pub output_format: Option<SpeechFormat>,
    /// The speed of speech (0.25 to 4.0).
    pub speed: Option<f32>,
    /// Abort signal for cancellation.
    pub abort_signal: Option<CancellationToken>,
}

impl GenerateSpeechOptions {
    /// Create new options with a model and text.
    pub fn new(model: impl Into<SpeechModel>, text: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            text: text.into(),
            ..Default::default()
        }
    }

    /// Set the voice.
    pub fn with_voice(mut self, voice: impl Into<SpeechVoice>) -> Self {
        self.voice = Some(voice.into());
        self
    }

    /// Set the output format.
    pub fn with_output_format(mut self, format: SpeechFormat) -> Self {
        self.output_format = Some(format);
        self
    }

    /// Set the speed.
    pub fn with_speed(mut self, speed: f32) -> Self {
        self.speed = Some(speed);
        self
    }

    /// Set the abort signal.
    pub fn with_abort_signal(mut self, signal: CancellationToken) -> Self {
        self.abort_signal = Some(signal);
        self
    }
}

/// Resolve a speech model reference to an actual model instance.
fn resolve_speech_model(model: SpeechModel) -> Result<Arc<dyn SpeechModelV4>, AIError> {
    match model {
        SpeechModel::V4(m) => Ok(m),
        SpeechModel::String(id) => {
            let provider = get_default_provider().ok_or_else(|| {
                AIError::InvalidArgument(
                    "No default provider set. Call set_default_provider() first or use a SpeechModel::V4 variant.".to_string(),
                )
            })?;
            provider
                .speech_model(&id)
                .map_err(|e| AIError::ProviderError(AISdkError::new(e.to_string())))
        }
    }
}

/// Generate speech audio from text.
///
/// This function converts text to speech using a speech synthesis model.
///
/// # Arguments
///
/// * `options` - The generation options including model, text, and voice settings.
///
/// # Returns
///
/// A `SpeechResult` containing the audio data and metadata.
///
/// # Example
///
/// ```ignore
/// use vercel_ai::{generate_speech, GenerateSpeechOptions};
///
/// let result = generate_speech(GenerateSpeechOptions {
///     model: "tts-1".into(),
///     text: "Hello, world!".to_string(),
///     voice: Some("alloy".into()),
///     ..Default::default()
/// }).await?;
///
/// // Save to file
/// std::fs::write("output.mp3", &result.audio.data)?;
/// ```
pub async fn generate_speech(options: GenerateSpeechOptions) -> Result<SpeechResult, AIError> {
    let model = resolve_speech_model(options.model)?;
    let model_id = model.model_id().to_string();

    // Build call options
    let mut call_options = SpeechModelV4CallOptions::new(&options.text);

    // Map voice
    if let Some(voice) = options.voice {
        call_options = call_options.with_voice(ProviderSpeechVoice::new(&voice.id));
    }

    // Map output format
    if let Some(format) = options.output_format {
        call_options = call_options.with_response_format(match format {
            SpeechFormat::Mp3 => ProviderSpeechFormat::Mp3,
            SpeechFormat::Opus => ProviderSpeechFormat::Opus,
            SpeechFormat::Aac => ProviderSpeechFormat::Aac,
            SpeechFormat::Flac => ProviderSpeechFormat::Flac,
            SpeechFormat::Wav => ProviderSpeechFormat::Wav,
            SpeechFormat::Pcm => ProviderSpeechFormat::Pcm,
        });
    }

    // Set speed
    if let Some(speed) = options.speed {
        call_options = call_options.with_speed(speed);
    }

    // Set abort signal
    if let Some(signal) = options.abort_signal {
        call_options = call_options.with_abort_signal(signal);
    }

    // Call the model
    let result = model.do_generate_speech(call_options).await?;

    // Check if audio was generated
    if result.audio.is_empty() {
        return Err(AIError::NoSpeechGenerated);
    }

    // Build the result
    let audio = GeneratedAudioFile::new(result.audio, result.content_type);
    let speech_result = SpeechResult::new(audio, model_id);

    Ok(speech_result)
}

#[cfg(test)]
#[path = "generate_speech.test.rs"]
mod tests;
