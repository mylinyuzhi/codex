//! Speech result types.

use serde::Deserialize;
use serde::Serialize;
use vercel_ai_provider::Warning;

/// Result of a `generate_speech` call.
#[derive(Debug)]
pub struct SpeechResult {
    /// The generated audio file.
    pub audio: GeneratedAudioFile,
    /// Warnings from the provider.
    pub warnings: Vec<Warning>,
    /// The model ID used.
    pub model_id: String,
}

impl SpeechResult {
    /// Create a new speech result.
    pub fn new(audio: GeneratedAudioFile, model_id: impl Into<String>) -> Self {
        Self {
            audio,
            warnings: Vec::new(),
            model_id: model_id.into(),
        }
    }

    /// Add warnings.
    pub fn with_warnings(mut self, warnings: Vec<Warning>) -> Self {
        self.warnings = warnings;
        self
    }
}

/// A generated audio file.
#[derive(Debug, Clone)]
pub struct GeneratedAudioFile {
    /// The audio data.
    pub data: Vec<u8>,
    /// The MIME type of the audio (e.g., "audio/mpeg", "audio/wav").
    pub media_type: String,
}

impl GeneratedAudioFile {
    /// Create a new audio file.
    pub fn new(data: Vec<u8>, media_type: impl Into<String>) -> Self {
        Self {
            data,
            media_type: media_type.into(),
        }
    }

    /// Create from MP3 data.
    pub fn mp3(data: Vec<u8>) -> Self {
        Self::new(data, "audio/mpeg")
    }

    /// Create from WAV data.
    pub fn wav(data: Vec<u8>) -> Self {
        Self::new(data, "audio/wav")
    }

    /// Create from Opus data.
    pub fn opus(data: Vec<u8>) -> Self {
        Self::new(data, "audio/opus")
    }

    /// Get the file extension based on the media type.
    pub fn extension(&self) -> &str {
        match self.media_type.as_str() {
            "audio/mpeg" | "audio/mp3" => "mp3",
            "audio/wav" | "audio/wave" => "wav",
            "audio/opus" => "opus",
            "audio/aac" => "aac",
            "audio/flac" => "flac",
            "audio/ogg" => "ogg",
            _ => "bin",
        }
    }

    /// Get the base64-encoded data.
    pub fn to_base64(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }
}

/// Voice options for speech synthesis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechVoice {
    /// The voice ID.
    pub id: String,
    /// The voice name (optional).
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

impl From<String> for SpeechVoice {
    fn from(id: String) -> Self {
        Self::new(id)
    }
}

impl From<&str> for SpeechVoice {
    fn from(id: &str) -> Self {
        Self::new(id)
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

impl SpeechFormat {
    /// Get the MIME type for this format.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Opus => "audio/opus",
            Self::Aac => "audio/aac",
            Self::Flac => "audio/flac",
            Self::Wav => "audio/wav",
            Self::Pcm => "audio/pcm",
        }
    }
}
