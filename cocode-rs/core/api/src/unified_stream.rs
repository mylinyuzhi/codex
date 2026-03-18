//! Unified stream abstraction over streaming and non-streaming responses.

use crate::AssistantContentPart;
use crate::FinishReason;
use crate::LanguageModelGenerateResult;
use crate::LanguageModelStreamPart;
use crate::StreamProcessor;
use crate::StreamSnapshot;
use crate::Usage;
use crate::error::ApiError;
use crate::error::Result;
use cocode_protocol::TokenUsage as ProtocolUsage;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::mpsc;

/// Type of result from the unified stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryResultType {
    /// Assistant message with completed content.
    Assistant,
    /// UI update event (delta).
    Event,
    /// Retry attempt indicator.
    Retry,
    /// Error occurred.
    Error,
    /// Stream is complete.
    Done,
}

/// Result from a unified stream iteration.
#[derive(Debug, Clone)]
pub struct StreamingQueryResult {
    /// Type of this result.
    pub result_type: QueryResultType,
    /// Completed content blocks (for Assistant type).
    pub content: Vec<AssistantContentPart>,
    /// Stream event for UI updates.
    pub event: Option<LanguageModelStreamPart>,
    /// Error if result_type is Error.
    pub error: Option<String>,
    /// Token usage (available on Done).
    pub usage: Option<ProtocolUsage>,
    /// Finish reason (available on Done).
    pub finish_reason: Option<FinishReason>,
    /// Whether the error is retryable (from provider StreamError).
    pub is_retryable: Option<bool>,
    /// Error code from the provider (from provider StreamError).
    pub error_code: Option<String>,
}

impl StreamingQueryResult {
    /// Create an assistant result with completed content.
    pub fn assistant(content: Vec<AssistantContentPart>) -> Self {
        Self {
            result_type: QueryResultType::Assistant,
            content,
            event: None,
            error: None,
            usage: None,
            finish_reason: None,
            is_retryable: None,
            error_code: None,
        }
    }

    /// Create an event result for UI updates.
    pub fn event(update: LanguageModelStreamPart) -> Self {
        Self {
            result_type: QueryResultType::Event,
            content: Vec::new(),
            event: Some(update),
            error: None,
            usage: None,
            finish_reason: None,
            is_retryable: None,
            error_code: None,
        }
    }

    /// Create a retry result.
    pub fn retry() -> Self {
        Self {
            result_type: QueryResultType::Retry,
            content: Vec::new(),
            event: None,
            error: None,
            usage: None,
            finish_reason: None,
            is_retryable: None,
            error_code: None,
        }
    }

    /// Create an error result.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            result_type: QueryResultType::Error,
            content: Vec::new(),
            event: None,
            error: Some(message.into()),
            usage: None,
            finish_reason: None,
            is_retryable: None,
            error_code: None,
        }
    }

    /// Create a done result.
    pub fn done(usage: Option<ProtocolUsage>, finish_reason: FinishReason) -> Self {
        Self {
            result_type: QueryResultType::Done,
            content: Vec::new(),
            event: None,
            error: None,
            usage,
            finish_reason: Some(finish_reason),
            is_retryable: None,
            error_code: None,
        }
    }

    /// Check if this is an assistant result with content.
    pub fn has_content(&self) -> bool {
        self.result_type == QueryResultType::Assistant && !self.content.is_empty()
    }

    /// Check if this result has tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.content
            .iter()
            .any(|b| matches!(b, AssistantContentPart::ToolCall(_)))
    }

    /// Get tool calls from this result.
    pub fn tool_calls(&self) -> Vec<&AssistantContentPart> {
        self.content
            .iter()
            .filter(|b| matches!(b, AssistantContentPart::ToolCall(_)))
            .collect()
    }
}

/// Inner implementation of unified stream.
enum UnifiedStreamInner {
    /// Streaming mode using StreamProcessor.
    Streaming(StreamProcessor),
    /// Non-streaming mode with a single response.
    NonStreaming(Option<LanguageModelGenerateResult>),
}

/// Unified abstraction for streaming and non-streaming API responses.
pub struct UnifiedStream {
    inner: UnifiedStreamInner,
    event_tx: Option<mpsc::Sender<LanguageModelStreamPart>>,
}

impl UnifiedStream {
    /// Create a unified stream from a streaming processor.
    pub fn from_stream(processor: StreamProcessor) -> Self {
        Self {
            inner: UnifiedStreamInner::Streaming(processor),
            event_tx: None,
        }
    }

    /// Create a unified stream from a non-streaming response.
    pub fn from_response(response: LanguageModelGenerateResult) -> Self {
        Self {
            inner: UnifiedStreamInner::NonStreaming(Some(response)),
            event_tx: None,
        }
    }

    /// Set an event sender for UI updates.
    pub fn with_event_sender(mut self, tx: mpsc::Sender<LanguageModelStreamPart>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Get the next result from the stream.
    pub async fn next(&mut self) -> Option<Result<StreamingQueryResult>> {
        match &mut self.inner {
            UnifiedStreamInner::Streaming(processor) => {
                Self::process_streaming_event(processor, &self.event_tx).await
            }
            UnifiedStreamInner::NonStreaming(opt) => Self::process_non_streaming(opt.take()),
        }
    }

    /// Process streaming events from the processor.
    async fn process_streaming_event(
        processor: &mut StreamProcessor,
        event_tx: &Option<mpsc::Sender<LanguageModelStreamPart>>,
    ) -> Option<Result<StreamingQueryResult>> {
        loop {
            match processor.next().await {
                Some(Ok((part, snapshot))) => {
                    // Send update to UI if configured
                    if let Some(tx) = event_tx
                        && let Err(e) = tx.send(part.clone()).await
                    {
                        tracing::debug!("Failed to send stream event to UI: {e}");
                    }

                    // Check for completed content
                    let completed = Self::check_for_completed_content(&part, snapshot);

                    if let Some(result) = completed {
                        return Some(Ok(result));
                    }

                    // Check if stream is done
                    if snapshot.is_complete {
                        let usage = Self::convert_usage(snapshot);
                        let finish_reason = snapshot
                            .finish_reason
                            .clone()
                            .unwrap_or_else(FinishReason::stop);
                        return Some(Ok(StreamingQueryResult::done(usage, finish_reason)));
                    }

                    // Continue to next event for deltas
                    if matches!(
                        part,
                        LanguageModelStreamPart::TextDelta { .. }
                            | LanguageModelStreamPart::ReasoningDelta { .. }
                            | LanguageModelStreamPart::ToolInputDelta { .. }
                    ) {
                        continue;
                    }
                }
                Some(Err(e)) => {
                    return Some(Err(ApiError::from(e)));
                }
                None => {
                    return None;
                }
            }
        }
    }

    /// Convert snapshot usage to protocol usage.
    fn convert_usage(snapshot: &StreamSnapshot) -> Option<ProtocolUsage> {
        snapshot.usage.as_ref().map(|u| ProtocolUsage {
            input_tokens: u.total_input_tokens() as i64,
            output_tokens: u.total_output_tokens() as i64,
            cache_read_tokens: u.input_tokens.cache_read.map(|v| v as i64),
            cache_creation_tokens: u.input_tokens.cache_write.map(|v| v as i64),
            reasoning_tokens: u.output_tokens.reasoning.map(|v| v as i64),
        })
    }

    /// Check if the event indicates completed content.
    fn check_for_completed_content(
        part: &LanguageModelStreamPart,
        snapshot: &StreamSnapshot,
    ) -> Option<StreamingQueryResult> {
        match part {
            LanguageModelStreamPart::TextEnd { .. } => {
                if !snapshot.text.is_empty() {
                    Some(StreamingQueryResult::assistant(vec![
                        AssistantContentPart::text(&snapshot.text),
                    ]))
                } else {
                    None
                }
            }
            LanguageModelStreamPart::ReasoningEnd { .. } => snapshot.reasoning.as_ref().map(|r| {
                StreamingQueryResult::assistant(vec![AssistantContentPart::reasoning(&r.content)])
            }),
            LanguageModelStreamPart::ToolCall(tc) => Some(StreamingQueryResult::assistant(vec![
                AssistantContentPart::tool_call(&tc.tool_call_id, &tc.tool_name, tc.input.clone()),
            ])),
            LanguageModelStreamPart::File(file) => Some(StreamingQueryResult::assistant(vec![
                AssistantContentPart::File(crate::FilePart::new(
                    crate::DataContent::Base64(file.data.clone()),
                    &file.media_type,
                )),
            ])),
            LanguageModelStreamPart::ReasoningFile(rf) => {
                Some(StreamingQueryResult::assistant(vec![
                    AssistantContentPart::ReasoningFile(crate::ReasoningFilePart::new(
                        crate::DataContent::Base64(rf.data.clone()),
                        &rf.media_type,
                    )),
                ]))
            }
            LanguageModelStreamPart::Source(source) => Some(StreamingQueryResult::assistant(vec![
                AssistantContentPart::Source(source.clone()),
            ])),
            // P20: Propagate stream errors from providers (e.g., Anthropic overloaded mid-stream).
            // P29: Preserve structured fields (is_retryable, code) for upstream classification.
            LanguageModelStreamPart::Error { error } => {
                let mut result = StreamingQueryResult::error(&error.message);
                result.is_retryable = Some(error.is_retryable);
                result.error_code = error.code.clone();
                Some(result)
            }
            _ => None,
        }
    }

    /// Handle non-streaming response.
    fn process_non_streaming(
        response: Option<LanguageModelGenerateResult>,
    ) -> Option<Result<StreamingQueryResult>> {
        let response = response?;

        let usage = convert_generate_usage(&response.usage);

        Some(Ok(StreamingQueryResult {
            result_type: QueryResultType::Assistant,
            content: response.content,
            event: None,
            error: None,
            usage: Some(usage),
            finish_reason: Some(response.finish_reason),
            is_retryable: None,
            error_code: None,
        }))
    }

    /// Collect all results into a single response.
    pub async fn collect(mut self) -> Result<CollectedResponse> {
        let mut content = Vec::new();
        let mut usage = None;
        let mut finish_reason = FinishReason::stop();

        while let Some(result) = self.next().await {
            let result = result?;

            match result.result_type {
                QueryResultType::Assistant => {
                    content.extend(result.content);
                    if result.usage.is_some() {
                        usage = result.usage;
                    }
                    if let Some(reason) = result.finish_reason {
                        finish_reason = reason;
                    }
                }
                QueryResultType::Done => {
                    usage = result.usage;
                    finish_reason = result.finish_reason.unwrap_or_else(FinishReason::stop);
                    break;
                }
                QueryResultType::Error => {
                    return Err(crate::error::api_error::StreamSnafu {
                        message: result.error.unwrap_or_default(),
                    }
                    .build());
                }
                _ => {}
            }
        }

        Ok(CollectedResponse {
            content,
            usage,
            finish_reason,
        })
    }
}

impl std::fmt::Debug for UnifiedStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mode = match &self.inner {
            UnifiedStreamInner::Streaming(_) => "Streaming",
            UnifiedStreamInner::NonStreaming(_) => "NonStreaming",
        };
        f.debug_struct("UnifiedStream")
            .field("mode", &mode)
            .finish_non_exhaustive()
    }
}

/// Collected response from a unified stream.
#[derive(Debug, Clone)]
pub struct CollectedResponse {
    /// All content blocks.
    pub content: Vec<AssistantContentPart>,
    /// Token usage.
    pub usage: Option<ProtocolUsage>,
    /// Finish reason.
    pub finish_reason: FinishReason,
}

impl CollectedResponse {
    /// Get the text content.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                AssistantContentPart::Text(tp) => Some(tp.text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Get thinking content if present.
    pub fn thinking(&self) -> Option<&str> {
        self.content.iter().find_map(|b| match b {
            AssistantContentPart::Reasoning(rp) => Some(rp.text.as_str()),
            _ => None,
        })
    }

    /// Get tool calls.
    pub fn tool_calls(&self) -> Vec<&AssistantContentPart> {
        self.content
            .iter()
            .filter(|b| matches!(b, AssistantContentPart::ToolCall(_)))
            .collect()
    }

    /// Check if the response has tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.content
            .iter()
            .any(|b| matches!(b, AssistantContentPart::ToolCall(_)))
    }

    /// Convert to a LanguageModelMessage for history.
    pub fn into_assistant_message(self) -> crate::LanguageModelMessage {
        crate::LanguageModelMessage::Assistant {
            content: self.content,
            provider_options: None,
        }
    }

    /// Convert to GenerateResult.
    pub fn into_generate_result(self) -> LanguageModelGenerateResult {
        let usage = self
            .usage
            .map(|u| {
                let mut usage = Usage::new(u.input_tokens as u64, u.output_tokens as u64);
                usage.input_tokens.cache_read = u.cache_read_tokens.map(|v| v as u64);
                usage.input_tokens.cache_write = u.cache_creation_tokens.map(|v| v as u64);
                usage.output_tokens.reasoning = u.reasoning_tokens.map(|v| v as u64);
                usage
            })
            .unwrap_or_default();
        LanguageModelGenerateResult::new(self.content, usage, self.finish_reason)
    }
}

/// Convert vercel-ai Usage to protocol TokenUsage.
pub fn convert_generate_usage(usage: &Usage) -> ProtocolUsage {
    ProtocolUsage {
        input_tokens: usage.total_input_tokens() as i64,
        output_tokens: usage.total_output_tokens() as i64,
        cache_read_tokens: usage.input_tokens.cache_read.map(|v| v as i64),
        cache_creation_tokens: usage.input_tokens.cache_write.map(|v| v as i64),
        reasoning_tokens: usage.output_tokens.reasoning.map(|v| v as i64),
    }
}

#[cfg(test)]
#[path = "unified_stream.test.rs"]
mod tests;
