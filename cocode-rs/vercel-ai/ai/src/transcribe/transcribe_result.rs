//! Transcription result types.

use serde::Deserialize;
use serde::Serialize;
use vercel_ai_provider::Warning;

/// Result of a `transcribe` call.
#[derive(Debug)]
pub struct TranscriptionResult {
    /// The transcribed text.
    pub text: String,
    /// Segments with timing information (if available).
    pub segments: Vec<TranscriptionSegment>,
    /// The detected language (if available).
    pub language: Option<String>,
    /// Duration in seconds (if available).
    pub duration_seconds: Option<f32>,
    /// Warnings from the provider.
    pub warnings: Vec<Warning>,
    /// The model ID used.
    pub model_id: String,
}

impl TranscriptionResult {
    /// Create a new transcription result.
    pub fn new(text: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            segments: Vec::new(),
            language: None,
            duration_seconds: None,
            warnings: Vec::new(),
            model_id: model_id.into(),
        }
    }

    /// Add segments.
    pub fn with_segments(mut self, segments: Vec<TranscriptionSegment>) -> Self {
        self.segments = segments;
        self
    }

    /// Set the language.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Set the duration.
    pub fn with_duration(mut self, duration: f32) -> Self {
        self.duration_seconds = Some(duration);
        self
    }

    /// Add warnings.
    pub fn with_warnings(mut self, warnings: Vec<Warning>) -> Self {
        self.warnings = warnings;
        self
    }

    /// Check if segments are available.
    pub fn has_segments(&self) -> bool {
        !self.segments.is_empty()
    }

    /// Get the word count.
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

/// A transcription segment with timing information.
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
}

impl TranscriptionSegment {
    /// Create a new segment.
    pub fn new(id: usize, start: f32, end: f32, text: impl Into<String>) -> Self {
        Self {
            id,
            start,
            end,
            text: text.into(),
        }
    }

    /// Get the duration in seconds.
    pub fn duration(&self) -> f32 {
        self.end - self.start
    }
}

/// A transcribed word with timing information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscribedWord {
    /// The word text.
    pub word: String,
    /// Start time in seconds.
    pub start: f32,
    /// End time in seconds.
    pub end: f32,
}

impl TranscribedWord {
    /// Create a new word.
    pub fn new(word: impl Into<String>, start: f32, end: f32) -> Self {
        Self {
            word: word.into(),
            start,
            end,
        }
    }

    /// Get the duration in seconds.
    pub fn duration(&self) -> f32 {
        self.end - self.start
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
