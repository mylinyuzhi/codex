//! Transcribe audio to text (speech-to-text).
//!
//! This module provides the `transcribe` function for transcribing audio
//! to text using transcription models.

#[allow(clippy::module_inception)]
mod transcribe;
mod transcribe_result;

pub use transcribe::AudioData;
pub use transcribe::TranscribeOptions;
pub use transcribe::TranscriptionModel;
pub use transcribe::transcribe;
pub use transcribe_result::TimestampGranularity;
pub use transcribe_result::TranscribedWord;
pub use transcribe_result::TranscriptionFormat;
pub use transcribe_result::TranscriptionResult;
pub use transcribe_result::TranscriptionSegment;
