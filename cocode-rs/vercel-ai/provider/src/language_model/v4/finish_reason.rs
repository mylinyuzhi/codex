//! Finish reason types for model responses.

use serde::Deserialize;
use serde::Serialize;
use std::fmt;

/// Unified finish reason values.
///
/// These are the standardized finish reasons used across different providers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnifiedFinishReason {
    /// The model finished normally.
    #[default]
    Stop,
    /// The maximum number of tokens was reached.
    Length,
    /// Content was filtered due to safety policies.
    ContentFilter,
    /// The model invoked a tool.
    ToolCalls,
    /// An error occurred.
    Error,
    /// Other/unspecified reason.
    Other,
}

impl UnifiedFinishReason {
    /// Check if this is a stop reason (normal completion).
    pub fn is_stop(&self) -> bool {
        matches!(self, Self::Stop)
    }

    /// Check if this is a length limit reason.
    pub fn is_length(&self) -> bool {
        matches!(self, Self::Length)
    }

    /// Check if this is a content filter reason.
    pub fn is_content_filter(&self) -> bool {
        matches!(self, Self::ContentFilter)
    }

    /// Check if this is a tool calls reason.
    pub fn is_tool_calls(&self) -> bool {
        matches!(self, Self::ToolCalls)
    }

    /// Check if this is an error reason.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }
}

impl fmt::Display for UnifiedFinishReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stop => write!(f, "stop"),
            Self::Length => write!(f, "length"),
            Self::ContentFilter => write!(f, "content-filter"),
            Self::ToolCalls => write!(f, "tool-calls"),
            Self::Error => write!(f, "error"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// The reason why a model response finished.
///
/// Contains both a unified finish reason and a raw finish reason from the provider.
/// The unified finish reason is used to provide a consistent finish reason across different providers.
/// The raw finish reason is used to provide the original finish reason from the provider.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishReason {
    /// Unified finish reason. This enables using the same finish reason across different providers.
    ///
    /// Can be one of the following:
    /// - `stop`: model generated stop sequence
    /// - `length`: model generated maximum number of tokens
    /// - `content-filter`: content filter violation stopped the model
    /// - `tool-calls`: model triggered tool calls
    /// - `error`: model stopped because of an error
    /// - `other`: model stopped for other reasons
    pub unified: UnifiedFinishReason,

    /// Raw finish reason from the provider.
    /// This is the original finish reason from the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl FinishReason {
    /// Create a new finish reason with the given unified value.
    pub fn new(unified: UnifiedFinishReason) -> Self {
        Self { unified, raw: None }
    }

    /// Create a finish reason with both unified and raw values.
    pub fn with_raw(unified: UnifiedFinishReason, raw: impl Into<String>) -> Self {
        Self {
            unified,
            raw: Some(raw.into()),
        }
    }

    /// Create a stop finish reason.
    pub fn stop() -> Self {
        Self::new(UnifiedFinishReason::Stop)
    }

    /// Create a length finish reason.
    pub fn length() -> Self {
        Self::new(UnifiedFinishReason::Length)
    }

    /// Create a content filter finish reason.
    pub fn content_filter() -> Self {
        Self::new(UnifiedFinishReason::ContentFilter)
    }

    /// Create a tool calls finish reason.
    pub fn tool_calls() -> Self {
        Self::new(UnifiedFinishReason::ToolCalls)
    }

    /// Create an error finish reason.
    pub fn error() -> Self {
        Self::new(UnifiedFinishReason::Error)
    }

    /// Create an other finish reason.
    pub fn other() -> Self {
        Self::new(UnifiedFinishReason::Other)
    }

    /// Set the raw finish reason.
    pub fn with_raw_value(mut self, raw: impl Into<String>) -> Self {
        self.raw = Some(raw.into());
        self
    }

    /// Check if the unified reason is stop.
    pub fn is_stop(&self) -> bool {
        self.unified.is_stop()
    }

    /// Check if the unified reason is length.
    pub fn is_length(&self) -> bool {
        self.unified.is_length()
    }

    /// Check if the unified reason is content filter.
    pub fn is_content_filter(&self) -> bool {
        self.unified.is_content_filter()
    }

    /// Check if the unified reason is tool calls.
    pub fn is_tool_calls(&self) -> bool {
        self.unified.is_tool_calls()
    }

    /// Check if the unified reason is error.
    pub fn is_error(&self) -> bool {
        self.unified.is_error()
    }
}

impl From<UnifiedFinishReason> for FinishReason {
    fn from(unified: UnifiedFinishReason) -> Self {
        Self::new(unified)
    }
}

#[cfg(test)]
#[path = "finish_reason.test.rs"]
mod tests;
