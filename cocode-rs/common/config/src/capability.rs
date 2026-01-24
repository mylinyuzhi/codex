//! Model capability and reasoning effort types.

use serde::{Deserialize, Serialize};

/// Model capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Basic text generation.
    TextGeneration,
    /// Streaming response support.
    Streaming,
    /// Vision/image input support.
    Vision,
    /// Audio input support.
    Audio,
    /// Tool/function calling support.
    ToolCalling,
    /// Embedding generation.
    Embedding,
    /// Extended thinking/reasoning support.
    ExtendedThinking,
    /// Structured output (JSON mode).
    StructuredOutput,
}

/// Reasoning effort level for models that support extended thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    /// Low reasoning effort.
    Low,
    /// Medium reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
}
