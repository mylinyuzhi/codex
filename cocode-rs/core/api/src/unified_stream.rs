//! Unified stream abstraction over streaming and non-streaming responses.
//!
//! This module provides [`UnifiedStream`] which provides a consistent interface
//! for both streaming and non-streaming API responses. The agent loop can use
//! the same code path regardless of whether streaming is enabled.

use crate::aggregation::AggregationState;
use crate::error::{ApiError, Result};
use cocode_protocol::TokenUsage as ProtocolUsage;
use hyper_sdk::{
    ContentBlock, FinishReason, GenerateResponse, StreamProcessor, StreamSnapshot, StreamUpdate,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Convert i64 to i32 with saturation and logging on overflow.
pub(crate) fn saturating_i64_to_i32(value: i64, field: &str) -> i32 {
    match i32::try_from(value) {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!(
                field,
                value,
                "Token count exceeds i32::MAX, clamping to i32::MAX"
            );
            if value > 0 { i32::MAX } else { i32::MIN }
        }
    }
}

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
    pub content: Vec<ContentBlock>,
    /// Stream event for UI updates.
    pub event: Option<StreamUpdate>,
    /// Error if result_type is Error.
    pub error: Option<String>,
    /// Token usage (available on Done).
    pub usage: Option<ProtocolUsage>,
    /// Finish reason (available on Done).
    pub finish_reason: Option<FinishReason>,
}

impl StreamingQueryResult {
    /// Create an assistant result with completed content.
    pub fn assistant(content: Vec<ContentBlock>) -> Self {
        Self {
            result_type: QueryResultType::Assistant,
            content,
            event: None,
            error: None,
            usage: None,
            finish_reason: None,
        }
    }

    /// Create an event result for UI updates.
    pub fn event(update: StreamUpdate) -> Self {
        Self {
            result_type: QueryResultType::Event,
            content: Vec::new(),
            event: Some(update),
            error: None,
            usage: None,
            finish_reason: None,
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
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    }

    /// Get tool calls from this result.
    pub fn tool_calls(&self) -> Vec<&ContentBlock> {
        self.content
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
            .collect()
    }
}

/// Inner implementation of unified stream.
enum UnifiedStreamInner {
    /// Streaming mode using StreamProcessor.
    Streaming(StreamProcessor),
    /// Non-streaming mode with a single response.
    NonStreaming(Option<GenerateResponse>),
    /// Already consumed.
    Consumed,
}

/// Unified abstraction for streaming and non-streaming API responses.
///
/// This provides a consistent interface for the agent loop to consume API
/// responses regardless of whether streaming is enabled. In streaming mode,
/// it yields results as content blocks complete. In non-streaming mode,
/// it yields a single result with all content.
pub struct UnifiedStream {
    inner: UnifiedStreamInner,
    state: AggregationState,
    event_tx: Option<mpsc::Sender<StreamUpdate>>,
}

impl UnifiedStream {
    /// Create a unified stream from a streaming processor.
    pub fn from_stream(processor: StreamProcessor) -> Self {
        Self {
            inner: UnifiedStreamInner::Streaming(processor),
            state: AggregationState::new(),
            event_tx: None,
        }
    }

    /// Create a unified stream from a non-streaming response.
    pub fn from_response(response: GenerateResponse) -> Self {
        Self {
            inner: UnifiedStreamInner::NonStreaming(Some(response)),
            state: AggregationState::new(),
            event_tx: None,
        }
    }

    /// Set an event sender for UI updates.
    pub fn with_event_sender(mut self, tx: mpsc::Sender<StreamUpdate>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Get the next result from the stream.
    ///
    /// Returns `None` when the stream is complete.
    pub async fn next(&mut self) -> Option<Result<StreamingQueryResult>> {
        // Check what mode we're in without borrowing self.inner
        let is_streaming = matches!(self.inner, UnifiedStreamInner::Streaming(_));
        let is_non_streaming = matches!(self.inner, UnifiedStreamInner::NonStreaming(_));

        if is_streaming {
            // Process streaming - we need to take and put back the processor
            let processor = match std::mem::replace(&mut self.inner, UnifiedStreamInner::Consumed) {
                UnifiedStreamInner::Streaming(p) => p,
                _ => unreachable!(),
            };
            let (result, processor) = self.process_streaming(processor).await;
            if let Some(p) = processor {
                self.inner = UnifiedStreamInner::Streaming(p);
            }
            result
        } else if is_non_streaming {
            // Process non-streaming
            let response = match std::mem::replace(&mut self.inner, UnifiedStreamInner::Consumed) {
                UnifiedStreamInner::NonStreaming(r) => r,
                _ => unreachable!(),
            };
            Self::process_non_streaming(response)
        } else {
            None
        }
    }

    /// Process streaming events.
    /// Returns the result and optionally the processor to put back.
    async fn process_streaming(
        &self,
        mut processor: StreamProcessor,
    ) -> (
        Option<Result<StreamingQueryResult>>,
        Option<StreamProcessor>,
    ) {
        loop {
            match processor.next().await {
                Some(Ok((update, snapshot))) => {
                    // Send update to UI if configured
                    if let Some(tx) = &self.event_tx {
                        if let Err(e) = tx.send(update.clone()).await {
                            tracing::debug!("Failed to send stream event to UI: {e}");
                        }
                    }

                    // Check for completed content
                    let completed = Self::check_for_completed_content(&update);

                    if let Some(result) = completed {
                        return (Some(Ok(result)), Some(processor));
                    }

                    // Check if stream is done
                    if snapshot.is_complete {
                        let usage = Self::convert_usage(&snapshot);
                        let finish_reason = snapshot.finish_reason.unwrap_or(FinishReason::Stop);
                        return (
                            Some(Ok(StreamingQueryResult::done(usage, finish_reason))),
                            None,
                        );
                    }

                    // Continue to next event for deltas
                    if update.is_delta() {
                        continue;
                    }
                }
                Some(Err(e)) => {
                    return (Some(Err(ApiError::from(e))), None);
                }
                None => {
                    return (None, None);
                }
            }
        }
    }

    /// Convert snapshot usage to protocol usage.
    fn convert_usage(snapshot: &StreamSnapshot) -> Option<ProtocolUsage> {
        snapshot.usage.as_ref().map(|u| ProtocolUsage {
            input_tokens: saturating_i64_to_i32(u.prompt_tokens, "prompt_tokens"),
            output_tokens: saturating_i64_to_i32(u.completion_tokens, "completion_tokens"),
            cache_read_tokens: u
                .cache_read_tokens
                .map(|v| saturating_i64_to_i32(v, "cache_read_tokens")),
            cache_creation_tokens: u
                .cache_creation_tokens
                .map(|v| saturating_i64_to_i32(v, "cache_creation_tokens")),
        })
    }

    /// Check if the update indicates completed content.
    fn check_for_completed_content(update: &StreamUpdate) -> Option<StreamingQueryResult> {
        match update {
            StreamUpdate::TextDone { text, .. } => {
                if !text.is_empty() {
                    Some(StreamingQueryResult::assistant(vec![ContentBlock::text(
                        text,
                    )]))
                } else {
                    None
                }
            }
            StreamUpdate::ThinkingDone {
                content, signature, ..
            } => Some(StreamingQueryResult::assistant(vec![
                ContentBlock::Thinking {
                    content: content.clone(),
                    signature: signature.clone(),
                },
            ])),
            StreamUpdate::ToolCallCompleted { tool_call, .. } => {
                Some(StreamingQueryResult::assistant(vec![
                    ContentBlock::tool_use(
                        &tool_call.id,
                        &tool_call.name,
                        tool_call.arguments.clone(),
                    ),
                ]))
            }
            _ => None,
        }
    }

    /// Handle non-streaming response.
    fn process_non_streaming(
        response: Option<GenerateResponse>,
    ) -> Option<Result<StreamingQueryResult>> {
        let response = response?;

        // Build usage
        let usage = response.usage.as_ref().map(|u| ProtocolUsage {
            input_tokens: saturating_i64_to_i32(u.prompt_tokens, "prompt_tokens"),
            output_tokens: saturating_i64_to_i32(u.completion_tokens, "completion_tokens"),
            cache_read_tokens: u
                .cache_read_tokens
                .map(|v| saturating_i64_to_i32(v, "cache_read_tokens")),
            cache_creation_tokens: u
                .cache_creation_tokens
                .map(|v| saturating_i64_to_i32(v, "cache_creation_tokens")),
        });

        // Return all content at once
        Some(Ok(StreamingQueryResult {
            result_type: QueryResultType::Assistant,
            content: response.content,
            event: None,
            error: None,
            usage,
            finish_reason: Some(response.finish_reason),
        }))
    }

    /// Get the current aggregation state.
    pub fn state(&self) -> &AggregationState {
        &self.state
    }

    /// Check if the stream is complete.
    pub fn is_complete(&self) -> bool {
        matches!(self.inner, UnifiedStreamInner::Consumed)
    }

    /// Collect all results into a single response.
    pub async fn collect(mut self) -> Result<CollectedResponse> {
        let mut content = Vec::new();
        let mut usage = None;
        let mut finish_reason = FinishReason::Stop;

        while let Some(result) = self.next().await {
            let result = result?;

            match result.result_type {
                QueryResultType::Assistant => {
                    content.extend(result.content);
                    // Capture usage from non-streaming responses
                    if result.usage.is_some() {
                        usage = result.usage;
                    }
                    if result.finish_reason.is_some() {
                        finish_reason = result.finish_reason.unwrap();
                    }
                }
                QueryResultType::Done => {
                    usage = result.usage;
                    finish_reason = result.finish_reason.unwrap_or(FinishReason::Stop);
                    break;
                }
                QueryResultType::Error => {
                    return Err(ApiError::stream(result.error.unwrap_or_default()));
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
        f.debug_struct("UnifiedStream")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// Collected response from a unified stream.
#[derive(Debug, Clone)]
pub struct CollectedResponse {
    /// All content blocks.
    pub content: Vec<ContentBlock>,
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
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Get thinking content if present.
    pub fn thinking(&self) -> Option<&str> {
        self.content.iter().find_map(|b| match b {
            ContentBlock::Thinking { content, .. } => Some(content.as_str()),
            _ => None,
        })
    }

    /// Get tool calls.
    pub fn tool_calls(&self) -> Vec<&ContentBlock> {
        self.content
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
            .collect()
    }

    /// Check if the response has tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_sdk::TokenUsage;

    fn make_response(text: &str) -> GenerateResponse {
        GenerateResponse {
            id: "resp_1".to_string(),
            content: vec![ContentBlock::text(text)],
            finish_reason: FinishReason::Stop,
            usage: Some(TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            }),
            model: "test-model".to_string(),
        }
    }

    #[tokio::test]
    async fn test_non_streaming_response() {
        let response = make_response("Hello, world!");
        let mut stream = UnifiedStream::from_response(response);

        let result = stream.next().await;
        assert!(result.is_some());

        let result = result.unwrap().unwrap();
        assert_eq!(result.result_type, QueryResultType::Assistant);
        assert!(!result.content.is_empty());

        // Should be consumed
        let result = stream.next().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_collect_non_streaming() {
        let response = make_response("Hello!");
        let stream = UnifiedStream::from_response(response);

        let collected = stream.collect().await.unwrap();
        assert_eq!(collected.text(), "Hello!");
        assert_eq!(collected.finish_reason, FinishReason::Stop);
        assert!(collected.usage.is_some());
    }

    #[test]
    fn test_streaming_query_result_constructors() {
        let assistant = StreamingQueryResult::assistant(vec![ContentBlock::text("test")]);
        assert!(assistant.has_content());

        let event = StreamingQueryResult::event(StreamUpdate::TextDelta {
            index: 0,
            delta: "hi".to_string(),
        });
        assert_eq!(event.result_type, QueryResultType::Event);

        let retry = StreamingQueryResult::retry();
        assert_eq!(retry.result_type, QueryResultType::Retry);

        let error = StreamingQueryResult::error("test error");
        assert_eq!(error.result_type, QueryResultType::Error);
        assert_eq!(error.error, Some("test error".to_string()));

        let done = StreamingQueryResult::done(None, FinishReason::Stop);
        assert_eq!(done.result_type, QueryResultType::Done);
    }

    #[test]
    fn test_tool_call_detection() {
        let result = StreamingQueryResult::assistant(vec![
            ContentBlock::text("Let me help"),
            ContentBlock::tool_use("call_1", "get_weather", serde_json::json!({"city": "NYC"})),
        ]);

        assert!(result.has_tool_calls());
        assert_eq!(result.tool_calls().len(), 1);
    }
}
