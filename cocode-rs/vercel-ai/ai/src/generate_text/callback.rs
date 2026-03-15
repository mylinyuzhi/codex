//! Callback types for generate_text and stream_text.
//!
//! This module provides callback types that allow users to hook into
//! the generation lifecycle.

use std::sync::Arc;

use vercel_ai_provider::FinishReason;
use vercel_ai_provider::ProviderMetadata;
use vercel_ai_provider::Source;
use vercel_ai_provider::Usage;

use super::generate_text_result::ToolCall;
use super::generated_file::GeneratedFile;
use super::step_result::StepResult;

use crate::types::LanguageModelRequestMetadata;
use crate::types::LanguageModelResponseMetadata;

/// Event data for the on_start callback.
#[derive(Debug, Clone)]
pub struct OnStartEvent {
    /// The model ID being used.
    pub model_id: String,
    /// The provider name.
    pub provider: Option<String>,
    /// System prompt (if any).
    pub system: Option<String>,
    /// Tool names available.
    pub tools: Vec<String>,
    /// Tool choice configuration description.
    pub tool_choice: Option<String>,
    /// Settings summary (e.g., temperature, max_tokens).
    pub settings: std::collections::HashMap<String, String>,
}

impl OnStartEvent {
    /// Create a new on_start event.
    pub fn new(model_id: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            provider: None,
            system: None,
            tools: Vec::new(),
            tool_choice: None,
            settings: std::collections::HashMap::new(),
        }
    }

    /// Set the provider name.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Set the system prompt.
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set the tool names.
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the tool choice description.
    pub fn with_tool_choice(mut self, tool_choice: impl Into<String>) -> Self {
        self.tool_choice = Some(tool_choice.into());
        self
    }

    /// Set settings summary.
    pub fn with_settings(mut self, settings: std::collections::HashMap<String, String>) -> Self {
        self.settings = settings;
        self
    }
}

/// Event data for the on_finish callback.
#[derive(Debug, Clone)]
pub struct OnFinishEvent<T = String> {
    /// The finish reason.
    pub finish_reason: FinishReason,
    /// Token usage for this response.
    pub usage: Usage,
    /// The generated output.
    pub output: T,
    /// Additional metadata.
    pub metadata: FinishEventMetadata,
    /// All steps taken during generation.
    pub steps: Vec<StepResult>,
    /// Cumulative usage across all steps.
    pub total_usage: Usage,
}

impl<T> OnFinishEvent<T> {
    /// Create a new on_finish event.
    pub fn new(finish_reason: FinishReason, usage: Usage, output: T) -> Self {
        let total_usage = usage.clone();
        Self {
            finish_reason,
            usage,
            output,
            metadata: FinishEventMetadata::default(),
            steps: Vec::new(),
            total_usage,
        }
    }

    /// Add metadata to the event.
    pub fn with_metadata(mut self, metadata: FinishEventMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Add steps to the event.
    pub fn with_steps(mut self, steps: Vec<StepResult>) -> Self {
        self.steps = steps;
        self
    }

    /// Set the total usage.
    pub fn with_total_usage(mut self, total_usage: Usage) -> Self {
        self.total_usage = total_usage;
        self
    }
}

/// Additional metadata for the finish event.
#[derive(Debug, Clone, Default)]
pub struct FinishEventMetadata {
    /// The model ID used.
    pub model_id: Option<String>,
    /// Response timestamp.
    pub timestamp: Option<String>,
    /// Response headers.
    pub headers: Option<std::collections::HashMap<String, String>>,
}

impl FinishEventMetadata {
    /// Create new metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the model ID.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    /// Set the timestamp.
    pub fn with_timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    /// Set the headers.
    pub fn with_headers(mut self, headers: std::collections::HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }
}

/// Event data for the on_step_start callback.
#[derive(Debug, Clone)]
pub struct OnStepStartEvent {
    /// The step number (0-indexed).
    pub step: u32,
    /// The tool call that triggered this step (if any).
    pub tool_call: Option<ToolCall>,
}

impl OnStepStartEvent {
    /// Create a new on_step_start event.
    pub fn new(step: u32) -> Self {
        Self {
            step,
            tool_call: None,
        }
    }

    /// Create an on_step_start event with a tool call.
    pub fn with_tool_call(mut self, tool_call: ToolCall) -> Self {
        self.tool_call = Some(tool_call);
        self
    }
}

/// Event data for the on_step_finish callback.
#[derive(Debug, Clone)]
pub struct OnStepFinishEvent {
    /// The step number (0-indexed).
    pub step: u32,
    /// The result of this step (contains content, reasoning, sources, files, metadata, etc.).
    pub result: StepResult,
    /// Reasoning text from this step.
    pub reasoning_text: Option<String>,
    /// Sources from this step.
    pub sources: Vec<Source>,
    /// Files from this step.
    pub files: Vec<GeneratedFile>,
    /// Provider metadata from this step.
    pub provider_metadata: Option<ProviderMetadata>,
    /// Request metadata.
    pub request: Option<LanguageModelRequestMetadata>,
    /// Response metadata.
    pub response: Option<LanguageModelResponseMetadata>,
}

impl OnStepFinishEvent {
    /// Create a new on_step_finish event.
    pub fn new(step: u32, result: StepResult) -> Self {
        let reasoning_text = if result.reasoning.is_empty() {
            None
        } else {
            Some(super::reasoning_output::reasoning_text(&result.reasoning))
        };
        let sources = result.sources.clone();
        let files = result.files.clone();
        let provider_metadata = result.provider_metadata.clone();
        let request = result.request.clone();
        let response = result.response.clone();

        Self {
            step,
            result,
            reasoning_text,
            sources,
            files,
            provider_metadata,
            request,
            response,
        }
    }
}

/// Event for tool call start.
#[derive(Debug, Clone)]
pub struct OnToolCallStartEvent {
    /// The tool call being started.
    pub tool_call: ToolCall,
    /// The step number.
    pub step: u32,
}

impl OnToolCallStartEvent {
    /// Create a new tool call start event.
    pub fn new(step: u32, tool_call: ToolCall) -> Self {
        Self { tool_call, step }
    }
}

/// Event for tool call finish.
#[derive(Debug, Clone)]
pub struct OnToolCallFinishEvent {
    /// The tool call that finished.
    pub tool_call: ToolCall,
    /// The result of the tool call.
    pub result: serde_json::Value,
    /// Whether the result is an error.
    pub is_error: bool,
    /// The step number.
    pub step: u32,
}

impl OnToolCallFinishEvent {
    /// Create a new tool call finish event.
    pub fn new(step: u32, tool_call: ToolCall, result: serde_json::Value, is_error: bool) -> Self {
        Self {
            tool_call,
            result,
            is_error,
            step,
        }
    }
}

/// Event for on_chunk callback in stream_text.
///
/// Contains the chunk type and its associated data.
#[derive(Debug, Clone)]
pub struct OnChunkEvent {
    /// The type of chunk (e.g., "text-delta", "reasoning-delta", "tool-call-start", etc.).
    pub chunk_type: String,
    /// Text delta content (for text-delta and reasoning-delta chunks).
    pub text: Option<String>,
    /// Tool call ID (for tool-related chunks).
    pub tool_call_id: Option<String>,
    /// Tool name (for tool-call-start chunks).
    pub tool_name: Option<String>,
}

impl OnChunkEvent {
    /// Create a text delta chunk event.
    pub fn text_delta(delta: impl Into<String>) -> Self {
        Self {
            chunk_type: "text-delta".to_string(),
            text: Some(delta.into()),
            tool_call_id: None,
            tool_name: None,
        }
    }

    /// Create a reasoning delta chunk event.
    pub fn reasoning_delta(delta: impl Into<String>) -> Self {
        Self {
            chunk_type: "reasoning-delta".to_string(),
            text: Some(delta.into()),
            tool_call_id: None,
            tool_name: None,
        }
    }

    /// Create a tool call start chunk event.
    pub fn tool_call_start(tool_call_id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        Self {
            chunk_type: "tool-call-start".to_string(),
            text: None,
            tool_call_id: Some(tool_call_id.into()),
            tool_name: Some(tool_name.into()),
        }
    }

    /// Create a tool call delta chunk event.
    pub fn tool_call_delta(tool_call_id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self {
            chunk_type: "tool-call-delta".to_string(),
            text: Some(delta.into()),
            tool_call_id: Some(tool_call_id.into()),
            tool_name: None,
        }
    }
}

/// Callbacks for generate_text.
#[derive(Default)]
pub struct GenerateTextCallbacks {
    /// Called when generation starts.
    pub on_start: Option<Arc<dyn Fn(OnStartEvent) + Send + Sync>>,
    /// Called when generation finishes.
    pub on_finish: Option<Arc<dyn Fn(OnFinishEvent) + Send + Sync>>,
    /// Called when a step starts.
    pub on_step_start: Option<Arc<dyn Fn(OnStepStartEvent) + Send + Sync>>,
    /// Called when a step finishes.
    pub on_step_finish: Option<Arc<dyn Fn(OnStepFinishEvent) + Send + Sync>>,
    /// Called when a tool call starts.
    pub on_tool_call_start: Option<Arc<dyn Fn(OnToolCallStartEvent) + Send + Sync>>,
    /// Called when a tool call finishes.
    pub on_tool_call_finish: Option<Arc<dyn Fn(OnToolCallFinishEvent) + Send + Sync>>,
}

impl GenerateTextCallbacks {
    /// Create new callbacks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the on_start callback.
    pub fn with_on_start<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnStartEvent) + Send + Sync + 'static,
    {
        self.on_start = Some(Arc::new(callback));
        self
    }

    /// Set the on_finish callback.
    pub fn with_on_finish<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnFinishEvent) + Send + Sync + 'static,
    {
        self.on_finish = Some(Arc::new(callback));
        self
    }

    /// Set the on_step_start callback.
    pub fn with_on_step_start<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnStepStartEvent) + Send + Sync + 'static,
    {
        self.on_step_start = Some(Arc::new(callback));
        self
    }

    /// Set the on_step_finish callback.
    pub fn with_on_step_finish<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnStepFinishEvent) + Send + Sync + 'static,
    {
        self.on_step_finish = Some(Arc::new(callback));
        self
    }

    /// Set the on_tool_call_start callback.
    pub fn with_on_tool_call_start<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnToolCallStartEvent) + Send + Sync + 'static,
    {
        self.on_tool_call_start = Some(Arc::new(callback));
        self
    }

    /// Set the on_tool_call_finish callback.
    pub fn with_on_tool_call_finish<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnToolCallFinishEvent) + Send + Sync + 'static,
    {
        self.on_tool_call_finish = Some(Arc::new(callback));
        self
    }
}

/// Callbacks for stream_text.
#[derive(Default)]
pub struct StreamTextCallbacks {
    /// Called when streaming starts.
    pub on_start: Option<Arc<dyn Fn(OnStartEvent) + Send + Sync>>,
    /// Called when streaming finishes.
    pub on_finish: Option<Arc<dyn Fn(OnFinishEvent) + Send + Sync>>,
    /// Called when a step starts.
    pub on_step_start: Option<Arc<dyn Fn(OnStepStartEvent) + Send + Sync>>,
    /// Called when a step finishes.
    pub on_step_finish: Option<Arc<dyn Fn(OnStepFinishEvent) + Send + Sync>>,
    /// Called when a chunk is received from the stream.
    pub on_chunk: Option<Arc<dyn Fn(OnChunkEvent) + Send + Sync>>,
    /// Called when a tool call starts.
    pub on_tool_call_start: Option<Arc<dyn Fn(OnToolCallStartEvent) + Send + Sync>>,
    /// Called when a tool call finishes.
    pub on_tool_call_finish: Option<Arc<dyn Fn(OnToolCallFinishEvent) + Send + Sync>>,
}

impl StreamTextCallbacks {
    /// Create new callbacks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the on_start callback.
    pub fn with_on_start<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnStartEvent) + Send + Sync + 'static,
    {
        self.on_start = Some(Arc::new(callback));
        self
    }

    /// Set the on_finish callback.
    pub fn with_on_finish<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnFinishEvent) + Send + Sync + 'static,
    {
        self.on_finish = Some(Arc::new(callback));
        self
    }

    /// Set the on_step_start callback.
    pub fn with_on_step_start<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnStepStartEvent) + Send + Sync + 'static,
    {
        self.on_step_start = Some(Arc::new(callback));
        self
    }

    /// Set the on_step_finish callback.
    pub fn with_on_step_finish<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnStepFinishEvent) + Send + Sync + 'static,
    {
        self.on_step_finish = Some(Arc::new(callback));
        self
    }

    /// Set the on_chunk callback.
    pub fn with_on_chunk<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnChunkEvent) + Send + Sync + 'static,
    {
        self.on_chunk = Some(Arc::new(callback));
        self
    }

    /// Set the on_tool_call_start callback.
    pub fn with_on_tool_call_start<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnToolCallStartEvent) + Send + Sync + 'static,
    {
        self.on_tool_call_start = Some(Arc::new(callback));
        self
    }

    /// Set the on_tool_call_finish callback.
    pub fn with_on_tool_call_finish<F>(mut self, callback: F) -> Self
    where
        F: Fn(OnToolCallFinishEvent) + Send + Sync + 'static,
    {
        self.on_tool_call_finish = Some(Arc::new(callback));
        self
    }
}

#[cfg(test)]
#[path = "callback.test.rs"]
mod tests;
