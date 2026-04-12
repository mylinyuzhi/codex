//! Accumulated state from a streaming response.

use vercel_ai_provider::language_model::v4::finish_reason::FinishReason;
use vercel_ai_provider::language_model::v4::usage::Usage;
use vercel_ai_provider::response_metadata::ResponseMetadata;
use vercel_ai_provider::shared::Warning;

/// Accumulated state from a streaming response.
///
/// Updated incrementally as stream parts arrive. Provides a consistent
/// view of all accumulated content at any point during streaming.
#[derive(Debug, Clone, Default)]
pub struct StreamSnapshot {
    /// Accumulated text content (all text segments concatenated).
    pub text: String,
    /// Reasoning/thinking content if present.
    pub reasoning: Option<ReasoningSnapshot>,
    /// Tool calls accumulated from the stream.
    pub tool_calls: Vec<ToolCallSnapshot>,
    /// Files received during streaming.
    pub files: Vec<FileSnapshot>,
    /// Sources/citations received during streaming.
    pub sources: Vec<SourceSnapshot>,
    /// Token usage (available after Finish event).
    pub usage: Option<Usage>,
    /// Finish reason (available after Finish event).
    pub finish_reason: Option<FinishReason>,
    /// Warnings from the provider (available after StreamStart).
    pub warnings: Vec<Warning>,
    /// Response metadata from the provider (model ID, request ID, headers).
    pub response_metadata: Option<ResponseMetadata>,
    /// Whether the stream has completed (Finish event received).
    pub is_complete: bool,
}

impl StreamSnapshot {
    /// Get completed tool calls (where the ToolCall event was received).
    pub fn completed_tool_calls(&self) -> Vec<&ToolCallSnapshot> {
        self.tool_calls.iter().filter(|tc| tc.is_complete).collect()
    }

    /// Get pending tool calls (input started but ToolCall not yet received).
    pub fn pending_tool_calls(&self) -> Vec<&ToolCallSnapshot> {
        self.tool_calls
            .iter()
            .filter(|tc| !tc.is_complete)
            .collect()
    }

    /// Check if any tool calls exist (complete or pending).
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    /// Check if reasoning content exists.
    pub fn has_reasoning(&self) -> bool {
        self.reasoning
            .as_ref()
            .is_some_and(|r| !r.content.is_empty())
    }
}

/// Accumulated reasoning/thinking content.
#[derive(Debug, Clone)]
pub struct ReasoningSnapshot {
    /// The reasoning segment ID.
    pub id: String,
    /// Accumulated reasoning text.
    pub content: String,
    /// Whether the reasoning segment is complete (ReasoningEnd received).
    pub is_complete: bool,
    /// Optional signature (provider-specific, e.g., Anthropic encrypted content).
    pub signature: Option<String>,
}

/// Accumulated tool call state.
#[derive(Debug, Clone)]
pub struct ToolCallSnapshot {
    /// The tool call ID.
    pub id: String,
    /// The tool name.
    pub tool_name: String,
    /// Accumulated input JSON string (may be partial during streaming).
    pub input_json: String,
    /// Whether ToolInputEnd has been received.
    pub is_input_complete: bool,
    /// Whether the final ToolCall event has been received.
    pub is_complete: bool,
}

/// A file received during streaming.
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    /// The file data (base64 or URL).
    pub data: String,
    /// The MIME type.
    pub media_type: String,
}

/// A source/citation received during streaming.
#[derive(Debug, Clone)]
pub struct SourceSnapshot {
    /// The source ID.
    pub id: String,
    /// The source URL.
    pub url: String,
    /// The source title.
    pub title: Option<String>,
}
