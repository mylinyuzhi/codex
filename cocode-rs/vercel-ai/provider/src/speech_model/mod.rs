//! Speech model module.
//!
//! This module provides speech (text-to-speech) model types organized by version.

pub mod v4;

// Re-export v4 types at this level
pub use v4::SpeechFormat;
pub use v4::SpeechModelV4;
pub use v4::SpeechModelV4CallOptions;
pub use v4::SpeechModelV4Result;
pub use v4::SpeechVoice;
